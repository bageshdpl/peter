//! M5: comments, signatures, and Word / Excel / ZIP exports.

mod common;

use std::{io::Read, time::Duration};

use axum::http::{StatusCode, header};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::{Api, token};
use serde_json::{Value, json};

const SCREEN: &[u8] = include_bytes!("fixtures/screen.jpg");

const BODY: &str = "# User requirements\n\nFrom the observed workflow [O1][O2] and SOP [S1].\n\n## Requirements\n\n| ID | Requirement | Evidence |\n| --- | --- | --- |\n| URS-001 | The system shall log in samples. | [O2] |\n| URS-002 | The system shall keep an audit trail. | [GAP: audit trail not observed] |\n\n- **Bold** point\n- Builds on [D:observe]\n";

/// A session with evidence (one step has a screenshot) and a drafted, edited
/// requirements document. Returns the owner's token and the session id.
async fn setup(api: &Api) -> (String, String) {
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, p) = api.call("POST", "/v1/projects", Some(&t), Some(json!({ "name": "LIMS", "system_name": "FakeLIMS 8.2" }))).await;
    let pid = p["id"].as_str().unwrap();
    api.upload(&format!("/v1/projects/{pid}/sources"), &t, "SOP-QA-014.txt", b"Samples logged in within 24 hours.").await;
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t),
        Some(json!({ "title": "LIMS IV", "validation_type": "initial", "project_id": pid }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    let obs = format!("/v1/sessions/{sid}/observations");
    api.call("POST", &obs, Some(&t), Some(json!({ "kind": "navigate", "url": "https://lims/samples", "page_title": "Samples" }))).await;
    api.call("POST", &obs, Some(&t), Some(json!({
        "kind": "click", "url": "https://lims/samples", "page_title": "Samples", "target_label": "Log in sample",
        "screenshot": format!("data:image/jpeg;base64,{}", STANDARD.encode(SCREEN)),
    }))).await;
    api.call("POST", &obs, Some(&t), Some(json!({ "kind": "input", "target_label": "Comment", "value": "=HYPERLINK(\"x\")" }))).await;

    for step in ["observe", "requirements"] {
        api.call("POST", &format!("/v1/sessions/{sid}/documents/{step}/draft"), Some(&t), Some(json!({ "provider": "sample" }))).await;
        for _ in 0..100 {
            let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/{step}"), Some(&t), None).await;
            if d["document"]["status"] == "ready" {
                break;
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    }
    let (s, b) = api.call("PUT", &format!("/v1/sessions/{sid}/documents/requirements"), Some(&t),
        Some(json!({ "body": BODY, "base_version": 1 }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    (t, sid)
}

fn unzip(bytes: &[u8]) -> zip::ZipArchive<std::io::Cursor<Vec<u8>>> {
    zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).expect("not a zip")
}

fn read(zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str) -> String {
    let mut s = String::new();
    zip.by_name(name).unwrap_or_else(|_| panic!("{name} missing")).read_to_string(&mut s).unwrap();
    s
}

#[tokio::test]
async fn comments_threads_and_resolution() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let path = format!("/v1/sessions/{sid}/documents/requirements/comments");
    let viewer = token(&api.add_member(&t, "qa@example.com", "viewer").await);

    // A viewer (QA reviewer) can raise a comment on quoted text.
    let (s, c) = api.call("POST", &path, Some(&viewer),
        Some(json!({ "body": "Which SOP version?", "quote": "SOP [S1]" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{c}");
    let cid = c["id"].as_str().unwrap().to_string();
    let (s, r) = api.call("POST", &path, Some(&t), Some(json!({ "body": "Version 3.", "parent_id": cid }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "body": "x", "parent_id": r["id"] }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST, "replies to replies are refused");
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "body": "   " }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (_, list) = api.call("GET", &path, Some(&t), None).await;
    let comments = list["comments"].as_array().unwrap();
    assert_eq!(comments.len(), 2);
    assert_eq!((comments[0]["quote"].as_str(), comments[0]["version"].as_i64()), (Some("SOP [S1]"), Some(2)));
    assert_eq!(comments[1]["parent_id"], cid.as_str());
    let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/requirements"), Some(&t), None).await;
    assert_eq!(doc["open_comments"], 1);

    // Another viewer cannot resolve it; the author and members can.
    let other_viewer = token(&api.add_member(&t, "v2@example.com", "viewer").await);
    let (s, _) = api.call("POST", &format!("/v1/comments/{cid}/resolve"), Some(&other_viewer), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("POST", &format!("/v1/comments/{cid}/resolve"), Some(&viewer), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = api.call("GET", &path, Some(&t), None).await;
    assert!(list["comments"][0]["resolved_at"].is_i64());
    let (s, _) = api.call("POST", &format!("/v1/comments/{cid}/reopen"), Some(&t), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/requirements"), Some(&t), None).await;
    assert_eq!(doc["open_comments"], 1);

    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("GET", &path, Some(&other), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = api.call("POST", &format!("/v1/comments/{cid}/resolve"), Some(&other), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, audit) = api.call("GET", "/v1/audit?limit=4", Some(&t), None).await;
    let actions: Vec<&str> = audit["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions[..2], ["comment.reopened", "comment.resolved"]);
}

#[tokio::test]
async fn signatures_need_the_password_and_belong_to_one_version() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let path = format!("/v1/sessions/{sid}/documents/requirements/signatures");

    let (s, b) = api.call("POST", &path, Some(&t), Some(json!({ "version": 2, "meaning": "approved", "password": "wrong-one" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("wrong_password")));
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 1, "meaning": "approved", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::CONFLICT, "only the current version");
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 2, "meaning": "liked", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 2, "meaning": "reviewed", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 2, "meaning": "approved", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 2, "meaning": "approved", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);

    let viewer = token(&api.add_member(&t, "qa@example.com", "viewer").await);
    let (s, _) = api.call("POST", &path, Some(&viewer), Some(json!({ "version": 2, "meaning": "reviewed", "password": "another-pass-1" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // A new version starts unsigned; the old signatures stay with version 2.
    let (_, saved) = api.call("PUT", &format!("/v1/sessions/{sid}/documents/requirements"), Some(&t),
        Some(json!({ "body": format!("{BODY}\nAdded line."), "base_version": 2 }))).await;
    let sigs = saved["signatures"].as_array().unwrap();
    assert_eq!(sigs.len(), 2);
    assert!(sigs.iter().all(|s| s["version"] == 2 && s["signer_name"] == "Test User"));
    assert_eq!(saved["document"]["current_version"], 3);

    let (_, audit) = api.call("GET", "/v1/audit?limit=10", Some(&t), None).await;
    let signed: Vec<&Value> = audit["events"].as_array().unwrap().iter().filter(|e| e["action"] == "document.signed").collect();
    assert_eq!(signed[0]["detail"]["meaning"], "approved");
}

#[tokio::test]
async fn word_export_has_cover_signatures_body_and_evidence() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    api.call("POST", &format!("/v1/sessions/{sid}/documents/requirements/signatures"), Some(&t),
        Some(json!({ "version": 2, "meaning": "approved", "password": "correct-horse-1" }))).await;

    let (s, headers, bytes) = api.raw_get(&format!("/v1/sessions/{sid}/documents/requirements/export/docx"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
    assert!(headers[header::CONTENT_DISPOSITION].to_str().unwrap().contains("User requirements and risk assessment v2.docx"));
    std::fs::write(std::env::temp_dir().join("peter-export-test.docx"), &bytes).unwrap();

    let mut zip = unzip(&bytes);
    let doc = read(&mut zip, "word/document.xml");
    for expected in [
        "User requirements and risk assessment", // cover title
        "FakeLIMS 8.2",
        "Approved",
        "Signed electronically in Peter",
        "URS-001",
        "GAP: audit trail not observed",
        "[see Observe and gather sources]",
        "Appendix A. Observed steps cited",
        "Clicked &quot;Log in sample&quot; on Samples",
        "Appendix B. Source files cited",
        "SOP-QA-014.txt",
        "Appendix C. Version history",
        "edit by a person",
    ] {
        assert!(doc.contains(expected), "missing {expected:?}");
    }
    assert!(doc.contains("open gap"), "gap warning on the cover");
    // The cited step's screenshot is embedded once.
    assert!(zip.by_name("word/media/image1.jpeg").is_ok());
    assert!(zip.by_name("word/media/image2.jpeg").is_err());

    // An older version can be exported too, and is marked as such.
    let (s, _, old) = api.raw_get(&format!("/v1/sessions/{sid}/documents/requirements/export/docx?version=1"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert!(read(&mut unzip(&old), "word/document.xml").contains("sample draft, no AI"));

    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&t), None).await;
    assert_eq!(audit["events"][0]["action"], "document.exported");
    assert_eq!(audit["events"][0]["detail"]["format"], "docx");
}

#[tokio::test]
async fn excel_export_has_one_sheet_per_table() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let (s, _, bytes) = api.raw_get(&format!("/v1/sessions/{sid}/documents/requirements/export/xlsx"), &t).await;
    assert_eq!(s, StatusCode::OK);
    std::fs::write(std::env::temp_dir().join("peter-export-test.xlsx"), &bytes).unwrap();
    let mut zip = unzip(&bytes);
    assert!(read(&mut zip, "xl/workbook.xml").contains(r#"<sheet name="Requirements""#));
    let sheet = read(&mut zip, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains("URS-002") && sheet.contains("[GAP: audit trail not observed]"));

    // The observe summary from the sample drafter has a table; a plain document does not.
    api.call("PUT", &format!("/v1/sessions/{sid}/documents/requirements"), Some(&t),
        Some(json!({ "body": "# No tables here", "base_version": 2 }))).await;
    let (s, _, _) = api.raw_get(&format!("/v1/sessions/{sid}/documents/requirements/export/xlsx"), &t).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    let (s, _, _) = api.raw_get(&format!("/v1/sessions/{sid}/documents/plan/export/docx"), &t).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn session_package_zip() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let (s, headers, bytes) = api.raw_get(&format!("/v1/sessions/{sid}/export"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(headers[header::CONTENT_TYPE], "application/zip");
    let mut zip = unzip(&bytes);
    let names: Vec<String> = (0..zip.len()).map(|i| zip.by_index(i).unwrap().name().unwrap().to_string()).collect();
    assert!(names.contains(&"documents/01 Observation summary.docx".to_string()), "{names:?}");
    assert!(names.contains(&"documents/03 User requirements and risk assessment.docx".to_string()), "{names:?}");
    assert!(names.contains(&"documents/03 User requirements and risk assessment tables.xlsx".to_string()), "{names:?}");
    let readme = read(&mut zip, "README.txt");
    assert!(readme.contains("User requirements and risk assessment: version 2, not signed; 1 open gap(s)"), "{readme}");
    let csv = read(&mut zip, "evidence/observed-steps.csv");
    assert!(csv.contains("\"O2\",\"click\""));
    // Cells that would run as spreadsheet formulas are defused.
    assert!(csv.contains("\"'=HYPERLINK(\"\"x\"\")\""), "{csv}");

    // Viewers may export; other workspaces may not.
    let viewer = token(&api.add_member(&t, "qa@example.com", "viewer").await);
    let (s, _, _) = api.raw_get(&format!("/v1/sessions/{sid}/export"), &viewer).await;
    assert_eq!(s, StatusCode::OK);
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _, _) = api.raw_get(&format!("/v1/sessions/{sid}/export"), &other).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
