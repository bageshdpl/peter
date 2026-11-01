//! Sharing, review requests, mentions and notifications.

mod common;

use std::time::Duration;

use axum::http::StatusCode;
use common::{Api, token};
use serde_json::{Value, json};

async fn draft(api: &Api, t: &str, sid: &str, step: &str) -> i64 {
    api.call("POST", &format!("/v1/sessions/{sid}/documents/{step}/draft"), Some(t), Some(json!({ "provider": "sample" }))).await;
    for _ in 0..100 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/{step}"), Some(t), None).await;
        if d["document"]["status"] == "ready" {
            return d["document"]["current_version"].as_i64().unwrap();
        }
        tokio::time::sleep(Duration::from_millis(30)).await;
    }
    panic!("draft did not finish");
}

async fn notes(api: &Api, t: &str) -> (Vec<String>, i64) {
    let (_, n) = api.call("GET", "/v1/notifications", Some(t), None).await;
    let texts = n["notifications"].as_array().unwrap().iter().map(|x| x["text"].as_str().unwrap().to_string()).collect();
    (texts, n["unread"].as_i64().unwrap())
}

fn user_id(session: &Value) -> String {
    session["user"]["id"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn review_requests_mentions_and_notifications() {
    let api = Api::new().await;
    let owner_s = api.signup("ana@example.com", "Acme").await;
    let ana = token(&owner_s);
    let bo_s = api.add_member(&ana, "bo@example.com", "member").await;
    let bo = token(&bo_s);
    let vic_s = api.add_member(&ana, "vic@example.com", "viewer").await;
    let vic = token(&vic_s);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&ana), Some(json!({ "title": "LIMS IV", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    let version = draft(&api, &ana, &sid, "requirements").await;
    let shares = format!("/v1/sessions/{sid}/shares");

    // Checks on who and what.
    let (s, b) = api.call("POST", &shares, Some(&ana), Some(json!({ "user_id": user_id(&owner_s) }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("self")));
    let (s, b) = api.call("POST", &shares, Some(&ana), Some(json!({ "user_id": user_id(&vic_s), "step_id": "requirements", "review": true }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("viewer")));
    let (s, b) = api.call("POST", &shares, Some(&ana), Some(json!({ "user_id": user_id(&bo_s), "step_id": "testing", "review": true }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("no_document")));
    let (s, _) = api.call("POST", &shares, Some(&vic), Some(json!({ "user_id": user_id(&bo_s) }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Ana shares the session with the viewer, and asks Bo to review the requirements.
    let (s, _) = api.call("POST", &shares, Some(&ana), Some(json!({ "user_id": user_id(&vic_s), "message": "FYI" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, r) = api.call("POST", &shares, Some(&ana),
        Some(json!({ "user_id": user_id(&bo_s), "step_id": "requirements", "review": true, "message": "Please sign by Friday" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let review_id = r["id"].as_str().unwrap().to_string();

    let (texts, unread) = notes(&api, &vic).await;
    assert_eq!((texts.len(), unread), (1, 1));
    assert!(texts[0].contains("shared “LIMS IV” with you") && texts[0].contains("FYI"), "{texts:?}");
    let (texts, _) = notes(&api, &bo).await;
    assert!(texts[0].contains("asked you to review and sign “Requirements and risk” in LIMS IV"), "{texts:?}");

    let (_, mine) = api.call("GET", "/v1/shared-with-me", Some(&bo), None).await;
    let mine = mine["shares"].as_array().unwrap();
    assert_eq!((mine.len(), mine[0]["kind"].as_str(), mine[0]["step_label"].as_str()),
        (1, Some("review"), Some("Requirements and risk")));
    let (_, on) = api.call("GET", &shares, Some(&vic), None).await;
    assert_eq!(on["shares"].as_array().unwrap().len(), 2);

    // Bo comments, naming Ana; Ana replies; Bo hears about the reply.
    let comments = format!("/v1/sessions/{sid}/documents/requirements/comments");
    let (_, c) = api.call("POST", &comments, Some(&bo), Some(json!({ "body": "@Test User is URS-003 high risk?" }))).await;
    let thread = c["id"].as_str().unwrap().to_string();
    let (texts, _) = notes(&api, &ana).await;
    assert!(texts[0].contains("mentioned you on") && texts[0].contains("URS-003"), "{texts:?}");
    api.call("POST", &comments, Some(&ana), Some(json!({ "body": "Yes, it is.", "parent_id": thread }))).await;
    let (texts, _) = notes(&api, &bo).await;
    assert!(texts[0].contains("replied on") && texts[0].contains("Yes, it is."), "{texts:?}");

    // Bo signs: the review request is done and Ana is told.
    let (s, b) = api.call("POST", &format!("/v1/sessions/{sid}/documents/requirements/signatures"), Some(&bo),
        Some(json!({ "version": version, "meaning": "reviewed", "password": "another-pass-1" }))).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let (texts, _) = notes(&api, &ana).await;
    assert!(texts[0].contains(&format!("signed version {version} of")) && texts[0].contains("as reviewed"), "{texts:?}");
    let (_, mine) = api.call("GET", "/v1/shared-with-me", Some(&bo), None).await;
    assert!(mine["shares"][0]["done_at"].is_i64());

    // Reading.
    let (_, n) = api.call("GET", "/v1/notifications?unread=true", Some(&bo), None).await;
    let first = n["notifications"][0]["id"].as_str().unwrap().to_string();
    let before = n["unread"].as_i64().unwrap();
    let (_, r) = api.call("POST", "/v1/notifications/read", Some(&bo), Some(json!({ "ids": [first] }))).await;
    assert_eq!(r["unread"].as_i64().unwrap(), before - 1);
    let (_, r) = api.call("POST", "/v1/notifications/read", Some(&bo), Some(json!({ "all": true }))).await;
    assert_eq!(r["unread"], 0);
    // Another person's notification cannot be touched.
    let (_, vn) = api.call("GET", "/v1/notifications", Some(&vic), None).await;
    let vid = vn["notifications"][0]["id"].as_str().unwrap().to_string();
    api.call("POST", "/v1/notifications/read", Some(&bo), Some(json!({ "ids": [vid] }))).await;
    assert_eq!(api.call("GET", "/v1/notifications/unread", Some(&vic), None).await.1["unread"], 1);

    // Only the sharer (or an admin) removes a share; only the recipient marks it done.
    assert_eq!(api.call("DELETE", &format!("/v1/shares/{review_id}"), Some(&bo), None).await.0, StatusCode::FORBIDDEN);
    assert_eq!(api.call("POST", &format!("/v1/shares/{review_id}/done"), Some(&ana), Some(json!({ "done": true }))).await.0, StatusCode::FORBIDDEN);
    assert_eq!(api.call("DELETE", &format!("/v1/shares/{review_id}"), Some(&ana), None).await.0, StatusCode::NO_CONTENT);
    let (_, mine) = api.call("GET", "/v1/shared-with-me", Some(&bo), None).await;
    assert_eq!(mine["shares"].as_array().unwrap().len(), 0);

    // Another workspace sees none of it.
    let other = token(&api.signup("zed@example.com", "Other").await);
    assert_eq!(api.call("GET", "/v1/notifications/unread", Some(&other), None).await.1["unread"], 0);
    let (s, _) = api.call("POST", &shares, Some(&other), Some(json!({ "user_id": user_id(&bo_s) }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, log) = api.call("GET", "/v1/audit", Some(&ana), None).await;
    let actions: Vec<&str> = log["events"].as_array().unwrap().iter().filter_map(|e| e["action"].as_str()).collect();
    assert!(["session.shared", "review.requested", "share.removed"].iter().all(|a| actions.contains(a)), "{actions:?}");
}
