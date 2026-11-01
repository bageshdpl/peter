//! The Library: regulations in scope, company templates, branding on exports.

mod common;

use std::{
    io::Read,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use common::{Api, token};
use peter_server::state::{AppState, Config};
use serde_json::{Value, json};

const LOGO: &[u8] = include_bytes!("fixtures/screen.jpg");

const TEMPLATE: &str = "ACME USER REQUIREMENTS SPECIFICATION\n\n1. Purpose\nThis URS defines what the system must do.\n\n2. Requirements\n| Req ID | Requirement | Priority | Verification |\n";

async fn wait_ready(api: &Api, t: &str, sid: &str, step: &str) -> Value {
    for _ in 0..100 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/{step}"), Some(t), None).await;
        if d["document"]["status"] != "drafting" {
            return d;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("draft did not finish");
}

fn read(bytes: &[u8], name: &str) -> Option<String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let mut s = String::new();
    zip.by_name(name).ok()?.read_to_string(&mut s).unwrap();
    Some(s)
}

#[tokio::test]
async fn regulations_are_chosen_by_admins() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let member = token(&api.add_member(&owner, "bo@example.com", "member").await);

    let (s, list) = api.call("GET", "/v1/library/standards", Some(&member), None).await;
    assert_eq!(s, StatusCode::OK);
    let ids: Vec<&str> = list["standards"].as_array().unwrap().iter().map(|s| s["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["part11", "annex11", "gamp5", "alcoa", "ichq9"]);
    assert_eq!(list["selected"], json!([]));

    let (s, _) = api.call("PUT", "/v1/library/standards", Some(&member), Some(json!({ "selected": ["part11"] }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, b) = api.call("PUT", "/v1/library/standards", Some(&owner), Some(json!({ "selected": ["part11", "iso9999"] }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unknown_standard")));
    let (s, b) = api.call("PUT", "/v1/library/standards", Some(&owner), Some(json!({ "selected": ["part11", "annex11", "part11"] }))).await;
    assert_eq!((s, b["selected"].clone()), (StatusCode::OK, json!(["annex11", "part11"])));
    let (_, list) = api.call("GET", "/v1/library/standards", Some(&member), None).await;
    assert_eq!(list["selected"], json!(["annex11", "part11"]));
}

#[tokio::test]
async fn templates_replace_each_other_per_step() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let (s, b) = api.upload_with("/v1/library/templates", &owner, "acme-urs.exe", b"MZ", &[("step_id", "requirements")]).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("file_type")));
    let (s, b) = api.upload_with("/v1/library/templates", &owner, "x.txt", TEMPLATE.as_bytes(), &[("step_id", "observe_nope")]).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unknown_step")));

    let (s, first) = api.upload_with("/v1/library/templates", &owner, "acme-urs-v1.md", TEMPLATE.as_bytes(),
        &[("step_id", "requirements"), ("name", "ACME URS template v1")]).await;
    assert_eq!(s, StatusCode::CREATED, "{first}");
    let (_, second) = api.upload_with("/v1/library/templates", &owner, "acme-urs-v2.md", TEMPLATE.as_bytes(),
        &[("step_id", "requirements"), ("name", "ACME URS template v2")]).await;
    let (_, list) = api.call("GET", "/v1/library/templates", Some(&owner), None).await;
    let templates = list["templates"].as_array().unwrap();
    assert_eq!(templates.len(), 1, "the newer template replaces the older");
    assert_eq!(templates[0]["name"], "ACME URS template v2");
    assert!(list["steps"].as_array().unwrap().iter().any(|s| s["step_id"] == "requirements" && s["document"] == "User requirements and risk assessment"));

    let id = second["id"].as_str().unwrap();
    let (_, one) = api.call("GET", &format!("/v1/library/templates/{id}"), Some(&owner), None).await;
    assert!(one["text"].as_str().unwrap().starts_with("ACME USER REQUIREMENTS SPECIFICATION"));
    let viewer = token(&api.add_member(&owner, "v@example.com", "viewer").await);
    let (s, _) = api.call("DELETE", &format!("/v1/library/templates/{id}"), Some(&viewer), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("DELETE", &format!("/v1/library/templates/{id}"), Some(&owner), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = api.call("GET", "/v1/library/templates", Some(&owner), None).await;
    assert!(list["templates"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn drafts_get_the_template_and_regulations() {
    #[derive(Clone, Default)]
    struct Seen(Arc<Mutex<Vec<Value>>>);
    let seen = Seen::default();
    let router = Router::new()
        .route("/chat/completions", post(|State(seen): State<Seen>, Json(body): Json<Value>| async move {
            seen.0.lock().unwrap().push(body);
            Json(json!({ "model": "m", "choices": [{ "message": { "content": "# URS\\n\\nDone [O1]." } }] }))
        }))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });

    let mut config = Config::for_tests();
    config.ai.openrouter_key = Some("k".into());
    config.ai.openrouter_base = base;
    let mail = Arc::new(common::CaptureMailer::default());
    let api = Api { app: peter_server::app(AppState::connect(config, mail.clone()).await.unwrap()), mail };

    let t = token(&api.signup("ana@example.com", "Acme").await);
    api.call("PUT", "/v1/library/standards", Some(&t), Some(json!({ "selected": ["part11", "alcoa"] }))).await;
    api.upload_with("/v1/library/templates", &t, "acme.md", TEMPLATE.as_bytes(), &[("step_id", "requirements"), ("name", "ACME URS")]).await;
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "S", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap();
    api.call("POST", &format!("/v1/sessions/{sid}/observations"), Some(&t), Some(json!({ "kind": "click", "target_label": "Go" }))).await;

    api.call("POST", &format!("/v1/sessions/{sid}/documents/requirements/draft"), Some(&t), Some(json!({ "provider": "openrouter" }))).await;
    wait_ready(&api, &t, sid, "requirements").await;
    let prompt = seen.0.lock().unwrap()[0]["messages"][1]["content"].as_str().unwrap().to_string();
    assert!(prompt.contains("# Company template") && prompt.contains("template \"ACME URS\""));
    assert!(prompt.contains("| Req ID | Requirement | Priority | Verification |"));
    assert!(prompt.contains("## 21 CFR Part 11 (US FDA)") && prompt.contains("- 11.10(e) Audit trail:"));
    assert!(prompt.contains("## ALCOA+ data integrity principles"));
    assert!(!prompt.contains("EU GMP Annex 11"));

    // Another step has no template; the regulations still apply.
    api.call("POST", &format!("/v1/sessions/{sid}/documents/plan/draft"), Some(&t), Some(json!({ "provider": "openrouter" }))).await;
    wait_ready(&api, &t, sid, "plan").await;
    let prompt = seen.0.lock().unwrap()[1]["messages"][1]["content"].as_str().unwrap().to_string();
    assert!(!prompt.contains("# Company template"));
    assert!(prompt.contains("## 21 CFR Part 11"));

    // The sample drafter says what is in use.
    api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&t), Some(json!({ "provider": "sample" }))).await;
    let sample = wait_ready(&api, &t, sid, "observe").await;
    assert!(sample["current"]["body"].as_str().unwrap().contains("21 CFR Part 11, ALCOA+ data integrity principles"));
}

#[tokio::test]
async fn branding_appears_on_word_exports() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "S", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    api.call("POST", &format!("/v1/sessions/{sid}/documents/requirements/draft"), Some(&t), Some(json!({ "provider": "sample" }))).await;
    wait_ready(&api, &t, &sid, "requirements").await;
    let export = format!("/v1/sessions/{sid}/documents/requirements/export/docx");

    // Without branding: no header part at all.
    let (_, _, plain) = api.raw_get(&export, &t).await;
    assert!(read(&plain, "word/header1.xml").is_none());
    assert!(!read(&plain, "[Content_Types].xml").unwrap().contains("header1.xml"));

    let (s, b) = api.call("PUT", "/v1/workspace/branding", Some(&t),
        Some(json!({ "company_name": "Acme Pharma Ltd", "doc_prefix": "acme csv-", "footer_text": "Confidential" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_field")));
    let (s, b) = api.call("PUT", "/v1/workspace/branding", Some(&t),
        Some(json!({ "company_name": "Acme Pharma Ltd", "doc_prefix": "acme-csv-", "footer_text": "Confidential" }))).await;
    assert_eq!((s, b["doc_prefix"].as_str()), (StatusCode::OK, Some("ACME-CSV-")));
    let (s, b) = api.upload("/v1/workspace/branding/logo", &t, "logo.txt", b"not an image").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("file_type")));
    let (s, b) = api.upload("/v1/workspace/branding/logo", &t, "logo.jpg", LOGO).await;
    assert_eq!((s, b["logo_type"].as_str()), (StatusCode::OK, Some("image/jpeg")), "{b}");
    let (s, _, logo) = api.raw_get("/v1/workspace/branding/logo", &t).await;
    assert_eq!((s, logo.as_slice()), (StatusCode::OK, LOGO));

    let (_, _, branded) = api.raw_get(&export, &t).await;
    let header = read(&branded, "word/header1.xml").expect("header");
    assert!(header.contains("Acme Pharma Ltd") && header.contains("rIdLogo"));
    assert!(read(&branded, "word/_rels/header1.xml.rels").unwrap().contains("media/logo.jpeg"));
    assert!(read(&branded, "[Content_Types].xml").unwrap().contains("header1.xml"));
    let session8: String = sid.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect::<String>().to_uppercase();
    let number = format!("ACME-CSV-{session8}-URS");
    assert!(read(&branded, "word/document.xml").unwrap().contains(&number), "document number on the cover");
    let footer = read(&branded, "word/footer1.xml").unwrap();
    assert!(footer.contains(&format!("Confidential · {number} · Version 1")), "{footer}");
    std::fs::write(std::env::temp_dir().join("peter-branded.docx"), &branded).unwrap();

    // Removing the logo keeps the company name in the header.
    api.call("DELETE", "/v1/workspace/branding/logo", Some(&t), None).await;
    let (_, _, no_logo) = api.raw_get(&export, &t).await;
    let header = read(&no_logo, "word/header1.xml").unwrap();
    assert!(header.contains("Acme Pharma Ltd") && !header.contains("rIdLogo"));
    assert!(read(&no_logo, "word/_rels/header1.xml.rels").is_none());

    let member = token(&api.add_member(&t, "bo@example.com", "member").await);
    let (s, _) = api.call("PUT", "/v1/workspace/branding", Some(&member),
        Some(json!({ "company_name": "x", "doc_prefix": "", "footer_text": "" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, audit) = api.call("GET", "/v1/audit?limit=5", Some(&t), None).await;
    assert!(audit["events"].as_array().unwrap().iter().any(|e| e["action"] == "library.logo_changed"));
}
