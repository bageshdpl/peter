mod common;

use axum::http::{StatusCode, header};
use common::{Api, token};
use serde_json::{Value, json};

async fn project(api: &Api, t: &str, name: &str) -> String {
    let (s, p) = api
        .call("POST", "/v1/projects", Some(t), Some(json!({ "name": name, "system_name": "LIMS 8.2", "vendor": "LabSoft" })))
        .await;
    assert_eq!(s, StatusCode::CREATED, "{p}");
    p["id"].as_str().unwrap().to_string()
}

fn actions(page: &Value) -> Vec<String> {
    page["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap().to_string()).collect()
}

#[tokio::test]
async fn validation_types_catalog() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (s, body) = api.call("GET", "/v1/validation-types", Some(&t), None).await;
    assert_eq!(s, StatusCode::OK);
    let types = body["types"].as_array().unwrap();
    assert_eq!(types.len(), 9);
    let initial = types.iter().find(|t| t["id"] == "initial").unwrap();
    assert_eq!(initial["available"], true);
    assert_eq!(initial["steps"].as_array().unwrap().len(), 10);
    assert!(types.iter().filter(|t| t["id"] != "initial").all(|t| t["available"] == false));
}

#[tokio::test]
async fn project_create_list_update_archive() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let id = project(&api, &owner, "LIMS rollout").await;

    let (_, list) = api.call("GET", "/v1/projects", Some(&owner), None).await;
    assert_eq!(list["projects"][0]["name"], "LIMS rollout");
    assert_eq!(list["projects"][0]["session_count"], 0);

    let (s, p) = api.call("PATCH", &format!("/v1/projects/{id}"), Some(&owner), Some(json!({ "name": "LIMS go-live" }))).await;
    assert_eq!((s, p["name"].as_str()), (StatusCode::OK, Some("LIMS go-live")));
    assert_eq!(p["vendor"], "LabSoft");

    let (s, b) = api.call("PATCH", &format!("/v1/projects/{id}"), Some(&owner), Some(json!({ "name": "  " }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_field")));

    let (s, p) = api.call("POST", &format!("/v1/projects/{id}/archive"), Some(&owner), None).await;
    assert_eq!(s, StatusCode::OK);
    assert!(p["archived_at"].is_i64());
    let (_, active) = api.call("GET", "/v1/projects", Some(&owner), None).await;
    assert!(active["projects"].as_array().unwrap().is_empty());
    let (_, archived) = api.call("GET", "/v1/projects?archived=true", Some(&owner), None).await;
    assert_eq!(archived["projects"].as_array().unwrap().len(), 1);

    // Archived projects are read-only until restored.
    let (s, _) = api.call("PATCH", &format!("/v1/projects/{id}"), Some(&owner), Some(json!({ "name": "x" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (s, _) = api.call("POST", &format!("/v1/projects/{id}/restore"), Some(&owner), None).await;
    assert_eq!(s, StatusCode::OK);

    let (_, audit) = api.call("GET", "/v1/audit", Some(&owner), None).await;
    assert_eq!(actions(&audit)[..4], ["project.restored", "project.archived", "project.updated", "project.created"]);
    let update = &audit["events"][2]["detail"];
    assert_eq!(update["name"], json!({ "from": "LIMS rollout", "to": "LIMS go-live" }));
}

#[tokio::test]
async fn project_permissions_and_isolation() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let id = project(&api, &owner, "LIMS").await;
    let viewer = token(&api.add_member(&owner, "view@example.com", "viewer").await);
    let member = token(&api.add_member(&owner, "mem@example.com", "member").await);

    // Viewers read; members write; only admins archive.
    let (s, _) = api.call("GET", &format!("/v1/projects/{id}"), Some(&viewer), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", "/v1/projects", Some(&viewer), Some(json!({ "name": "x" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("PATCH", &format!("/v1/projects/{id}"), Some(&member), Some(json!({ "vendor": "New" }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", &format!("/v1/projects/{id}/archive"), Some(&member), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Another workspace cannot see or touch it.
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("GET", &format!("/v1/projects/{id}"), Some(&other), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = api.call("POST", "/v1/sessions", Some(&other),
        Some(json!({ "title": "x", "validation_type": "initial", "project_id": id }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (_, list) = api.call("GET", "/v1/projects", Some(&other), None).await;
    assert!(list["projects"].as_array().unwrap().is_empty());
}

#[tokio::test]
async fn sessions_create_file_move_archive() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let lims = project(&api, &t, "LIMS").await;
    let mes = project(&api, &t, "MES").await;

    let (s, created) = api.call("POST", "/v1/sessions", Some(&t),
        Some(json!({ "title": "LIMS initial validation", "validation_type": "initial", "project_id": lims }))).await;
    assert_eq!(s, StatusCode::CREATED, "{created}");
    assert_eq!(created["session"]["current_step"], "observe");
    assert_eq!(created["session"]["project_name"], "LIMS");
    assert_eq!(created["type"]["id"], "initial");
    let sid = created["session"]["id"].as_str().unwrap().to_string();

    // Unavailable and unknown types are refused.
    let (s, b) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "x", "validation_type": "change" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("type_unavailable")));
    let (s, _) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "x", "validation_type": "nope" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // An unfiled session, then filed into MES.
    let (_, loose) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "Quick look", "validation_type": "initial" }))).await;
    let loose_id = loose["session"]["id"].as_str().unwrap().to_string();
    let (_, unfiled) = api.call("GET", "/v1/sessions?unfiled=true", Some(&t), None).await;
    assert_eq!(unfiled["sessions"].as_array().unwrap().len(), 1);
    let (s, moved) = api.call("PATCH", &format!("/v1/sessions/{loose_id}"), Some(&t), Some(json!({ "project_id": mes }))).await;
    assert_eq!((s, moved["session"]["project_name"].as_str()), (StatusCode::OK, Some("MES")));
    let (_, unfiled) = api.call("GET", "/v1/sessions?unfiled=true", Some(&t), None).await;
    assert!(unfiled["sessions"].as_array().unwrap().is_empty());
    // `null` unfiles it again; leaving the field out keeps it.
    let (_, kept) = api.call("PATCH", &format!("/v1/sessions/{loose_id}"), Some(&t), Some(json!({ "title": "Renamed" }))).await;
    assert_eq!(kept["session"]["project_id"], mes.as_str());
    let (_, out) = api.call("PATCH", &format!("/v1/sessions/{loose_id}"), Some(&t), Some(json!({ "project_id": null }))).await;
    assert!(out["session"]["project_id"].is_null());

    // Project counts and filtering.
    let (_, in_lims) = api.call("GET", &format!("/v1/sessions?project_id={lims}"), Some(&t), None).await;
    assert_eq!(in_lims["sessions"].as_array().unwrap().len(), 1);
    let (_, p) = api.call("GET", &format!("/v1/projects/{lims}"), Some(&t), None).await;
    assert_eq!(p["session_count"], 1);

    // Archive hides it and blocks edits.
    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/archive"), Some(&t), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, p) = api.call("GET", &format!("/v1/projects/{lims}"), Some(&t), None).await;
    assert_eq!(p["session_count"], 0);
    let (s, _) = api.call("PATCH", &format!("/v1/sessions/{sid}"), Some(&t), Some(json!({ "title": "y" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
    let (_, archived) = api.call("GET", "/v1/sessions?archived=true", Some(&t), None).await;
    assert_eq!(archived["sessions"].as_array().unwrap().len(), 1);

    // Sessions cannot be added to an archived project.
    api.call("POST", &format!("/v1/projects/{mes}/archive"), Some(&t), None).await;
    let (s, _) = api.call("POST", "/v1/sessions", Some(&t),
        Some(json!({ "title": "x", "validation_type": "initial", "project_id": mes }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn sources_upload_download_remove() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let id = project(&api, &t, "LIMS").await;
    let path = format!("/v1/projects/{id}/sources");

    let (s, up) = api.upload(&path, &t, "C:\\docs\\Vendor Manual é.pdf", b"%PDF-1.7 hello").await;
    assert_eq!(s, StatusCode::CREATED, "{up}");
    let src = &up["source"];
    assert_eq!(src["filename"], "Vendor Manual é.pdf");
    assert_eq!(src["content_type"], "application/pdf");
    assert_eq!(src["size_bytes"], 14);
    let sid = src["id"].as_str().unwrap().to_string();

    let (s, headers, bytes) = api.raw_get(&format!("{path}/{sid}/file"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(bytes, b"%PDF-1.7 hello");
    assert_eq!(headers[header::CONTENT_TYPE], "application/pdf");
    let disposition = headers[header::CONTENT_DISPOSITION].to_str().unwrap();
    assert!(disposition.starts_with("attachment;") && disposition.contains("UTF-8''Vendor%20Manual%20%C3%A9.pdf"), "{disposition}");

    // Wrong types, empty files and oversized files are refused.
    let (s, b) = api.upload(&path, &t, "run.exe", b"MZ").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("file_type")));
    let (s, _) = api.upload(&path, &t, "empty.txt", b"").await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let big = vec![b'a'; 1024 * 1024 + 10];
    let (s, _) = api.upload(&path, &t, "big.txt", &big).await;
    assert_eq!(s, StatusCode::PAYLOAD_TOO_LARGE);

    let (_, list) = api.call("GET", &path, Some(&t), None).await;
    assert_eq!(list["sources"].as_array().unwrap().len(), 1);
    let (_, p) = api.call("GET", &format!("/v1/projects/{id}"), Some(&t), None).await;
    assert_eq!(p["source_count"], 1);

    // Viewers can download but not upload or remove; other workspaces see nothing.
    let viewer = token(&api.add_member(&t, "view@example.com", "viewer").await);
    let (s, _, _) = api.raw_get(&format!("{path}/{sid}/file"), &viewer).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.upload(&path, &viewer, "notes.txt", b"hi").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _, _) = api.raw_get(&format!("{path}/{sid}/file"), &other).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, _) = api.call("DELETE", &format!("{path}/{sid}"), Some(&t), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _, _) = api.raw_get(&format!("{path}/{sid}/file"), &t).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, audit) = api.call("GET", "/v1/audit", Some(&t), None).await;
    let acts = actions(&audit);
    assert!(acts.contains(&"source.uploaded".to_string()) && acts.contains(&"source.removed".to_string()));
}
