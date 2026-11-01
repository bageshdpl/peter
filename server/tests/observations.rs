mod common;

use std::time::Duration;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use common::{Api, token};
use http_body_util::BodyExt;
use serde_json::{Value, json};
use tower::ServiceExt;

/// The smallest byte string the server accepts as a JPEG (it checks the magic bytes).
const JPEG: &[u8] = &[0xFF, 0xD8, 0xFF, 0xE0, 0x00, 0x10, b'J', b'F', b'I', b'F', 0x00, 0xFF, 0xD9];

async fn setup(api: &Api) -> (String, String) {
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "LIMS run", "validation_type": "initial" }))).await;
    (t, s["session"]["id"].as_str().unwrap().to_string())
}

fn step(kind: &str, label: &str) -> Value {
    json!({ "kind": kind, "url": "https://lims.example.com/samples", "page_title": "Samples", "target_label": label })
}

#[tokio::test]
async fn extension_connect_code_is_single_use() {
    let api = Api::new().await;
    let (t, _) = setup(&api).await;
    let (s, code) = api.call("POST", "/v1/auth/extension-code", Some(&t), None).await;
    assert_eq!(s, StatusCode::CREATED);
    let code = code["code"].as_str().unwrap();

    let (s, ext) = api.call("POST", "/v1/auth/extension-exchange", None, Some(json!({ "code": code }))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(ext["user"]["email"], "ana@example.com");
    let (s, _) = api.call("GET", "/v1/auth/me", Some(ext["access_token"].as_str().unwrap()), None).await;
    assert_eq!(s, StatusCode::OK);

    let (s, b) = api.call("POST", "/v1/auth/extension-exchange", None, Some(json!({ "code": code }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_code")));
    // Codes need a signed-in user to create.
    let (s, _) = api.call("POST", "/v1/auth/extension-code", None, None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn record_list_screenshot_and_remove() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let path = format!("/v1/sessions/{sid}/observations");

    let mut first = step("click", "Log in sample");
    first["screenshot"] = json!(format!("data:image/jpeg;base64,{}", STANDARD.encode(JPEG)));
    let (s, o1) = api.call("POST", &path, Some(&t), Some(first)).await;
    assert_eq!(s, StatusCode::CREATED, "{o1}");
    assert_eq!((o1["seq"].as_i64(), o1["has_screenshot"].as_bool()), (Some(1), Some(true)));

    let mut typed = step("input", "Password");
    typed["value"] = json!("••••••");
    typed["masked"] = json!(true);
    let (_, o2) = api.call("POST", &path, Some(&t), Some(typed)).await;
    assert_eq!((o2["seq"].as_i64(), o2["masked"].as_bool()), (Some(2), Some(true)));
    let (_, o3) = api.call("POST", &path, Some(&t), Some(json!({ "kind": "note", "note": "Batch 42 used as test data." }))).await;
    assert_eq!(o3["seq"], 3);

    let (_, list) = api.call("GET", &path, Some(&t), None).await;
    assert_eq!(list["observations"].as_array().unwrap().len(), 3);
    let (_, after) = api.call("GET", &format!("{path}?after_seq=2"), Some(&t), None).await;
    assert_eq!(after["observations"].as_array().unwrap().len(), 1);

    let id1 = o1["id"].as_str().unwrap();
    let (s, headers, bytes) = api.raw_get(&format!("{path}/{id1}/screenshot"), &t).await;
    assert_eq!((s, bytes.as_slice()), (StatusCode::OK, JPEG));
    assert_eq!(headers[header::CONTENT_TYPE], "image/jpeg");
    let id2 = o2["id"].as_str().unwrap();
    let (s, _, _) = api.raw_get(&format!("{path}/{id2}/screenshot"), &t).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // Removing keeps numbering; new steps continue after the highest number.
    let (s, _) = api.call("DELETE", &format!("{path}/{id2}"), Some(&t), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, list) = api.call("GET", &path, Some(&t), None).await;
    let seqs: Vec<i64> = list["observations"].as_array().unwrap().iter().map(|o| o["seq"].as_i64().unwrap()).collect();
    assert_eq!(seqs, [1, 3]);
    let (_, o4) = api.call("POST", &path, Some(&t), Some(step("navigate", ""))).await;
    assert_eq!(o4["seq"], 4);

    let (_, audit) = api.call("GET", "/v1/audit", Some(&t), None).await;
    assert_eq!(audit["events"][0]["action"], "observation.removed");
    assert_eq!(audit["events"][0]["detail"]["seq"], 2);
}

#[tokio::test]
async fn bad_steps_are_refused() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let path = format!("/v1/sessions/{sid}/observations");

    let (s, b) = api.call("POST", &path, Some(&t), Some(step("hover", "x"))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_kind")));
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "kind": "note", "note": "  " }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // A PNG label on JPEG bytes, and plain text, are not screenshots.
    let mut wrong = step("click", "x");
    wrong["screenshot"] = json!(format!("data:image/png;base64,{}", STANDARD.encode(JPEG)));
    let (s, b) = api.call("POST", &path, Some(&t), Some(wrong)).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_screenshot")));
    let mut text = step("click", "x");
    text["screenshot"] = json!("data:text/html;base64,PHNjcmlwdD4=");
    let (s, _) = api.call("POST", &path, Some(&t), Some(text)).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Viewers watch but cannot record; archived sessions take no steps; other workspaces see nothing.
    let viewer = token(&api.add_member(&t, "view@example.com", "viewer").await);
    let (s, _) = api.call("GET", &path, Some(&viewer), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", &path, Some(&viewer), Some(step("click", "x"))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("POST", &path, Some(&other), Some(step("click", "x"))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    api.call("POST", &format!("/v1/sessions/{sid}/archive"), Some(&t), None).await;
    let (s, _) = api.call("POST", &path, Some(&t), Some(step("click", "x"))).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

/// Reads the SSE body until an event with `name` arrives, returning its data.
async fn next_event(body: &mut Body, name: &str) -> Value {
    let mut buf = String::new();
    loop {
        let frame = tokio::time::timeout(Duration::from_secs(5), body.frame()).await.expect("no event in time");
        let frame = frame.expect("stream ended").unwrap();
        if let Some(data) = frame.data_ref() {
            buf.push_str(std::str::from_utf8(data).unwrap());
        }
        while let Some(end) = buf.find("\n\n") {
            let block: String = buf.drain(..end + 2).collect();
            if block.lines().any(|l| l == format!("event: {name}")) {
                let data = block.lines().find_map(|l| l.strip_prefix("data: ")).unwrap_or("null");
                return serde_json::from_str(data).unwrap();
            }
        }
    }
}

#[tokio::test]
async fn live_events_reach_the_session_page() {
    let api = Api::new().await;
    let (t, sid) = setup(&api).await;
    let (_, other_session) =
        api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "Other", "validation_type": "initial" }))).await;
    let other_sid = other_session["session"]["id"].as_str().unwrap();

    let req = Request::builder()
        .uri(format!("/v1/sessions/{sid}/events"))
        .header(header::AUTHORIZATION, format!("Bearer {t}"))
        .body(Body::empty())
        .unwrap();
    let res = api.app.clone().oneshot(req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    assert_eq!(res.headers()[header::CONTENT_TYPE], "text/event-stream");
    let mut body = res.into_body();
    next_event(&mut body, "ready").await;

    // A step in another session must not appear in this stream.
    api.call("POST", &format!("/v1/sessions/{other_sid}/observations"), Some(&t), Some(step("click", "Elsewhere"))).await;
    let (_, created) = api.call("POST", &format!("/v1/sessions/{sid}/observations"), Some(&t), Some(step("click", "Save"))).await;
    let got = next_event(&mut body, "observation").await;
    assert_eq!(got["target_label"], "Save");

    let id = created["id"].as_str().unwrap();
    api.call("DELETE", &format!("/v1/sessions/{sid}/observations/{id}"), Some(&t), None).await;
    let removed = next_event(&mut body, "observation_removed").await;
    assert_eq!(removed["id"], id);
}
