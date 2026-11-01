//! Controls: SOP statements found in a project's files, and a session's
//! coverage of them and of the regulations in scope.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{Api, token};
use serde_json::{Value, json};

const SOP: &str = "SOP-012 Computerised systems\n\n5 Responsibilities\n5.1 The system owner shall approve every change before release.\n\
5.2 Users must complete training before they get access.\nThis paragraph only describes things.\n";

const REQS: &str = "# User requirements\n\n| ID | Requirement | Risk |\n| --- | --- | --- |\n\
| URS-001 | The system shall keep an audit trail (21 CFR 11.10(e); Annex 11, clause 9). | High |\n\
| URS-002 | Changes shall be approved before release (CTL-001). | Medium |\n\
| URS-003 | Data shall be backed up daily. | Low |\n";

const TESTS: &str = "# Test scripts\n\n### TS-001 Audit trail\n\nRequirements covered: URS-001\n\n| Step | Pass/Fail |\n| --- | --- |\n| 1 | Pass |\n\n\
### TS-002 Change approval\n\nRequirements covered: URS-002\n\n| Step | Pass/Fail |\n| --- | --- |\n| 1 | Fail |\n";

async fn put_doc(api: &Api, t: &str, sid: &str, step: &str, body: &str) {
    api.call("POST", &format!("/v1/sessions/{sid}/documents/{step}/draft"), Some(t), Some(json!({ "provider": "sample" }))).await;
    let mut version = 0;
    for _ in 0..100 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/{step}"), Some(t), None).await;
        if d["document"]["status"] == "ready" {
            version = d["document"]["current_version"].as_i64().unwrap();
            break;
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    let (s, b) = api.call("PUT", &format!("/v1/sessions/{sid}/documents/{step}"), Some(t),
        Some(json!({ "body": body, "base_version": version }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

fn row<'a>(cov: &'a Value, key: &str) -> &'a Value {
    cov["coverage"]["rows"].as_array().unwrap().iter().find(|r| r["key"] == key).unwrap_or_else(|| panic!("no {key} in {cov}"))
}

fn enc(key: &str) -> String {
    key.replace(' ', "%20").replace('(', "%28").replace(')', "%29")
}

#[tokio::test]
async fn sop_controls_are_found_numbered_and_removed() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, p) = api.call("POST", "/v1/projects", Some(&t), Some(json!({ "name": "LIMS" }))).await;
    let pid = p["id"].as_str().unwrap();
    let (_, up) = api.upload(&format!("/v1/projects/{pid}/sources"), &t, "SOP-012.txt", SOP.as_bytes()).await;
    let src = up["source"]["id"].as_str().unwrap();

    let (s, found) = api.call("POST", &format!("/v1/projects/{pid}/sources/{src}/controls"), Some(&t), None).await;
    assert_eq!(s, StatusCode::CREATED, "{found}");
    assert_eq!(found["added"], json!(["CTL-001", "CTL-002"]));
    let c = &found["controls"][0];
    assert_eq!((c["reference"].as_str(), c["section"].as_str(), c["source_filename"].as_str()), (Some("CTL-001"), Some("5.1"), Some("SOP-012.txt")));

    // Running it again adds nothing new.
    let (_, again) = api.call("POST", &format!("/v1/projects/{pid}/sources/{src}/controls"), Some(&t), None).await;
    assert_eq!((again["added"].as_array().unwrap().len(), again["skipped"].as_i64()), (0, Some(2)));

    // By hand; numbers are never reused after a removal.
    let (s, _) = api.call("POST", &format!("/v1/projects/{pid}/controls"), Some(&t),
        Some(json!({ "text": "Backups shall be restored and checked every quarter.", "section": "7" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, _) = api.call("POST", &format!("/v1/projects/{pid}/controls"), Some(&t),
        Some(json!({ "text": "backups SHALL be restored, and checked every quarter" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let id3 = {
        let (_, l) = api.call("GET", &format!("/v1/projects/{pid}/controls"), Some(&t), None).await;
        l["controls"][2]["id"].as_str().unwrap().to_string()
    };
    assert_eq!(api.call("DELETE", &format!("/v1/projects/{pid}/controls/{id3}"), Some(&t), None).await.0, StatusCode::NO_CONTENT);
    let (_, added) = api.call("POST", &format!("/v1/projects/{pid}/controls"), Some(&t), Some(json!({ "text": "Another control shall exist." }))).await;
    assert_eq!(added["added"], json!(["CTL-004"]));

    // A file with no requirement statements says so.
    let (_, plain) = api.upload(&format!("/v1/projects/{pid}/sources"), &t, "notes.txt", b"Nothing required here.").await;
    let plain = plain["source"]["id"].as_str().unwrap();
    let (s, b) = api.call("POST", &format!("/v1/projects/{pid}/sources/{plain}/controls"), Some(&t), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::UNPROCESSABLE_ENTITY, Some("no_controls")));

    // Viewers can read but not change them.
    let viewer = token(&api.add_member(&t, "vic@example.com", "viewer").await);
    assert_eq!(api.call("GET", &format!("/v1/projects/{pid}/controls"), Some(&viewer), None).await.0, StatusCode::OK);
    assert_eq!(api.call("POST", &format!("/v1/projects/{pid}/controls"), Some(&viewer), Some(json!({ "text": "x shall y z w." }))).await.0,
        StatusCode::FORBIDDEN);

    let (_, log) = api.call("GET", "/v1/audit", Some(&t), None).await;
    let actions: Vec<&str> = log["events"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    assert!(actions.contains(&"controls.added") && actions.contains(&"controls.removed"), "{actions:?}");
}

#[tokio::test]
async fn session_coverage_follows_requirements_and_decisions() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    api.call("PUT", "/v1/library/standards", Some(&t), Some(json!({ "selected": ["part11", "annex11", "gamp5"] }))).await;
    let (_, p) = api.call("POST", "/v1/projects", Some(&t), Some(json!({ "name": "LIMS" }))).await;
    let pid = p["id"].as_str().unwrap();
    api.call("POST", &format!("/v1/projects/{pid}/controls"), Some(&t), Some(json!({ "text": "Changes shall be approved before release." }))).await;
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t),
        Some(json!({ "title": "LIMS IV", "validation_type": "initial", "project_id": pid }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();

    // Before any requirements, nothing is covered.
    let (s, cov) = api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&t), None).await;
    assert_eq!(s, StatusCode::OK, "{cov}");
    let total = cov["coverage"]["summary"]["controls"].as_i64().unwrap();
    assert_eq!(cov["coverage"]["summary"]["not_covered"].as_i64().unwrap(), total);
    assert!(cov["coverage"]["rows"].as_array().unwrap().iter().all(|r| !r["group"].as_str().unwrap().starts_with("GAMP")));

    put_doc(&api, &t, &sid, "requirements", REQS).await;
    put_doc(&api, &t, &sid, "testing", TESTS).await;
    let (_, cov) = api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&t), None).await;
    assert_eq!(row(&cov, "part11:Audit trail")["status"], "met");
    assert_eq!(row(&cov, "part11:Audit trail")["found"], json!(["URS-001"]));
    assert_eq!(row(&cov, "annex11:Audit trails")["status"], "met");
    assert_eq!(row(&cov, "project:CTL-001")["status"], "failed");
    assert_eq!(row(&cov, "project:CTL-001")["tests"], json!(["TS-002"]));

    // Link by hand, and mark one not applicable with a reason.
    let (s, b) = api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("annex11:Data storage")), Some(&t),
        Some(json!({ "requirements": ["URS-003"] }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("part11:Open systems")), Some(&t),
        Some(json!({ "not_applicable": true }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_field")));
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("part11:Open systems")), Some(&t),
        Some(json!({ "not_applicable": true, "reason": "Closed system: only company staff use it." }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("part11:Training")), Some(&t),
        Some(json!({ "requirements": ["URS-1; DROP"] }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_requirement")));
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("gamp5:Custom applications")), Some(&t),
        Some(json!({ "requirements": [] }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, cov) = api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&t), None).await;
    let storage = row(&cov, "annex11:Data storage");
    assert_eq!((storage["status"].as_str(), storage["linked"].clone()), (Some("untested"), json!(["URS-003"])));
    let open = row(&cov, "part11:Open systems");
    assert_eq!((open["status"].as_str(), open["reason"].as_str()), (Some("not_applicable"), Some("Closed system: only company staff use it.")));
    assert_eq!(cov["coverage"]["summary"]["not_applicable"], 1);
    assert_eq!(cov["requirements"].as_array().unwrap().len(), 3);

    // Clearing a decision removes it.
    api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("annex11:Data storage")), Some(&t), Some(json!({}))).await;
    let (_, cov) = api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&t), None).await;
    assert_eq!(row(&cov, "annex11:Data storage")["status"], "not_covered");

    // Excel export.
    let (s, headers, bytes) = api.raw_get(&format!("/v1/sessions/{sid}/controls/export"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert!(headers["content-disposition"].to_str().unwrap().contains("controls.xlsx"));
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut strings = String::new();
    std::io::Read::read_to_string(&mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(), &mut strings).unwrap();
    assert!(strings.contains("CTL-001") && strings.contains("Not applicable") && strings.contains("Closed system"));

    // Viewers can look, not decide; another workspace sees nothing.
    let viewer = token(&api.add_member(&t, "vic@example.com", "viewer").await);
    assert_eq!(api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&viewer), None).await.0, StatusCode::OK);
    assert_eq!(api.call("PUT", &format!("/v1/sessions/{sid}/controls/{}", enc("part11:Training")), Some(&viewer),
        Some(json!({ "requirements": ["URS-001"] }))).await.0, StatusCode::FORBIDDEN);
    let other = token(&api.signup("bob@example.com", "Other").await);
    assert_eq!(api.call("GET", &format!("/v1/sessions/{sid}/controls"), Some(&other), None).await.0, StatusCode::NOT_FOUND);
}
