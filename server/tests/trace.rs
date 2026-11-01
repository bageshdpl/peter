//! The traceability matrix, built from a session's documents.

mod common;

use std::{io::Read, time::Duration};

use axum::http::StatusCode;
use common::{Api, token};
use serde_json::json;

const REQS: &str = "# User requirements\n\n| ID | Requirement | Category | GxP critical | Risk | Evidence |\n| --- | --- | --- | --- | --- | --- |\n| URS-001 | The system shall require sign-in. | Security | Yes | High | [O1] |\n| URS-002 | The system shall log in samples. | Functional | Yes | Medium | [O2] |\n| URS-003 | The system shall keep an audit trail. | Audit trail | Yes | High | [GAP: not observed] |\n";

const TESTS: &str = "# Test scripts\n\n### TS-001 Sign-in\n\nRequirements covered: URS-001\n\n| Step | Action | Expected | Actual | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Sign in | Home | Home | [O1] | Pass |\n\n### TS-002 Sample login\n\nRequirements covered: URS-002\n\n| Step | Action | Expected | Actual | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Log in sample | Logged | Logged | [O2] | Pass |\n";

/// Creates a step's document with the given text (sample draft, then an edit).
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

async fn setup(api: &Api) -> (String, String) {
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "LIMS IV", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    for label in ["Sign in", "Log in sample"] {
        api.call("POST", &format!("/v1/sessions/{sid}/observations"), Some(&t),
            Some(json!({ "kind": "click", "url": "https://lims/x", "target_label": label }))).await;
    }
    (t, sid)
}

#[tokio::test]
async fn matrix_reads_the_documents() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;

    // Before any requirements: an empty matrix that says why.
    let (s, empty) = api.call("GET", &format!("/v1/sessions/{sid}/trace"), Some(&t), None).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(empty["matrix"]["summary"]["requirements"], 0);
    assert!(empty["matrix"]["warnings"][0].as_str().unwrap().contains("No requirements document"));
    let (s, b) = api.call("POST", &format!("/v1/sessions/{sid}/trace/write"), Some(&t), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("no_requirements")));

    put_doc(&api, &t, &sid, "requirements", REQS).await;
    put_doc(&api, &t, &sid, "testing", TESTS).await;
    let (_, got) = api.call("GET", &format!("/v1/sessions/{sid}/trace"), Some(&t), None).await;
    let m = &got["matrix"];
    assert_eq!(m["summary"]["requirements"], 3);
    assert_eq!(m["summary"]["passed"], 2);
    assert_eq!(m["summary"]["not_tested"], 1);
    assert_eq!(m["summary"]["high_risk_open"], 1);
    assert_eq!(m["rows"][2]["status"], "not_tested");
    assert_eq!(m["rows"][0]["tests"], json!(["TS-001"]));
    let from: Vec<&str> = got["built_from"].as_array().unwrap().iter().map(|b| b["step_id"].as_str().unwrap()).collect();
    assert_eq!(from, ["requirements", "testing"]);
}

#[tokio::test]
async fn writing_the_matrix_makes_a_generated_version() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    put_doc(&api, &t, &sid, "requirements", REQS).await;
    put_doc(&api, &t, &sid, "testing", TESTS).await;

    let (s, w) = api.call("POST", &format!("/v1/sessions/{sid}/trace/write"), Some(&t), None).await;
    assert_eq!(s, StatusCode::CREATED, "{w}");
    assert_eq!(w["version"], 1);
    let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/traceability"), Some(&t), None).await;
    let current = &doc["current"];
    assert_eq!((current["origin"].as_str(), current["model"].as_str()), (Some("generated"), Some("peter-trace")));
    let body = current["body"].as_str().unwrap();
    assert!(body.contains("| URS-001 | The system shall require sign-in. | high | — | TS-001 (passed) | Passed | [O1] |"), "{body}");
    assert!(body.contains("[GAP: no test covers URS-003]"));
    // Its citations are checked like any other document: all of them exist.
    assert_eq!(current["citations"]["unknown"], json!([]));
    assert!(current["citations"]["cited"].as_array().unwrap().contains(&json!("D:requirements")));

    // Fixing the gap and rebuilding gives version 2 with everything passed.
    let fixed = format!("{TESTS}\n### TS-003 Audit trail\n\nRequirements covered: URS-003\n\n| Step | Action | Expected | Actual | Evidence | Pass/Fail |\n| --- | --- | --- | --- | --- | --- |\n| 1 | Edit | Entry | Entry | [O2] | Pass |\n");
    let (_, testing) = api.call("GET", &format!("/v1/sessions/{sid}/documents/testing"), Some(&t), None).await;
    api.call("PUT", &format!("/v1/sessions/{sid}/documents/testing"), Some(&t),
        Some(json!({ "body": fixed, "base_version": testing["document"]["current_version"] }))).await;
    let (_, w2) = api.call("POST", &format!("/v1/sessions/{sid}/trace/write"), Some(&t), None).await;
    assert_eq!(w2["version"], 2);
    let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/traceability"), Some(&t), None).await;
    assert!(doc["current"]["body"].as_str().unwrap().contains("None. Every requirement is covered by a passed test."));

    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&t), None).await;
    assert_eq!(audit["events"][0]["action"], "document.generated");
    assert_eq!(audit["events"][0]["detail"]["open"], 0);

    // The Word export says how this version was made.
    let (_, _, docx) = api.raw_get(&format!("/v1/sessions/{sid}/documents/traceability/export/docx"), &t).await;
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(docx)).unwrap();
    let mut xml = String::new();
    zip.by_name("word/document.xml").unwrap().read_to_string(&mut xml).unwrap();
    assert!(xml.contains("built by Peter from the session"));
}

#[tokio::test]
async fn excel_export_and_permissions() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let (s, _, _) = api.raw_get(&format!("/v1/sessions/{sid}/trace/export"), &t).await;
    assert_eq!(s, StatusCode::UNPROCESSABLE_ENTITY);
    put_doc(&api, &t, &sid, "requirements", REQS).await;

    let (s, _, bytes) = api.raw_get(&format!("/v1/sessions/{sid}/trace/export"), &t).await;
    assert_eq!(s, StatusCode::OK);
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes)).unwrap();
    let mut wb = String::new();
    zip.by_name("xl/workbook.xml").unwrap().read_to_string(&mut wb).unwrap();
    for sheet in ["Traceability", "Tests", "Open items"] {
        assert!(wb.contains(&format!(r#"name="{sheet}""#)), "{wb}");
    }

    let viewer = token(&api.add_member(&t, "qa@example.com", "viewer").await);
    let (s, _) = api.call("GET", &format!("/v1/sessions/{sid}/trace"), Some(&viewer), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/trace/write"), Some(&viewer), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("GET", &format!("/v1/sessions/{sid}/trace"), Some(&other), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    api.call("POST", &format!("/v1/sessions/{sid}/archive"), Some(&t), None).await;
    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/trace/write"), Some(&t), None).await;
    assert_eq!(s, StatusCode::CONFLICT);
}
