//! The company's writing style: samples, a learnt or hand-written guide, and
//! drafts that follow it.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Json, Router, extract::State, http::StatusCode, routing::post};
use common::{Api, token};
use peter_server::state::{AppState, Config};
use serde_json::{Value, json};

const GUIDE: &str = "## Voice and wording\n- Short, impersonal sentences in the present tense.\n\n## Test scripts\n- Each step: Action, Expected result, Actual result, Pass/Fail.";

const SAMPLE: &str = "ACME-OQ-112 Operational qualification\n\n1. Purpose\nThis protocol verifies that the sample login function operates as intended.\n\n\
2. Test steps\nStep 1. Action: Log in as an analyst. Expected result: The home page is displayed. Actual result: As expected. Pass/Fail: Pass.\n\
Step 2. Action: Register a sample. Expected result: A sample ID is assigned. Actual result: As expected. Pass/Fail: Pass.\n";

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<Value>>>);

async fn fake() -> (String, Seen) {
    let seen = Seen::default();
    let router = Router::new()
        .route("/chat/completions", post(|State(seen): State<Seen>, Json(body): Json<Value>| async move {
            let learning = body["messages"][0]["content"].as_str().unwrap_or("").contains("house style");
            seen.0.lock().unwrap().push(body);
            let content = if learning { format!("```markdown\n{GUIDE}\n```") } else { "# Test scripts\n\n[GAP: nothing observed]".to_string() };
            Json(json!({ "model": "test/model", "choices": [{ "message": { "content": content } }],
                "usage": { "prompt_tokens": 800, "completion_tokens": 120 } }))
        }))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{addr}"), seen)
}

#[tokio::test]
async fn style_is_learnt_edited_and_used_in_drafts() {
    let (base, seen) = fake().await;
    let mut config = Config::for_tests();
    config.ai.openrouter_key = Some("server-key-0000".into());
    config.ai.openrouter_base = base;
    let mail = Arc::new(common::CaptureMailer::default());
    let api = Api { app: peter_server::app(AppState::connect(config, mail.clone()).await.unwrap()), mail };
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let member = token(&api.add_member(&t, "bo@example.com", "member").await);

    // Nothing to learn from yet.
    let (s, b) = api.call("POST", "/v1/library/style/learn", Some(&t), Some(json!({ "provider": "openrouter" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("no_samples")));

    // Samples: admins only, readable files with enough text.
    let (s, _) = api.upload("/v1/library/style/samples", &member, "oq.txt", SAMPLE.as_bytes()).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, b) = api.upload("/v1/library/style/samples", &t, "oq.exe", b"MZ").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("file_type")));
    let (s, b) = api.upload("/v1/library/style/samples", &t, "short.txt", b"Too short to learn from.").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unreadable")));
    let (s, b) = api.upload("/v1/library/style/samples", &t, "ACME-OQ-112.txt", SAMPLE.as_bytes()).await;
    assert_eq!(s, StatusCode::CREATED, "{b}");
    let sample_id = b["id"].as_str().unwrap().to_string();

    // Learning sends the samples' text, and keeps the guide without the code fence.
    let (s, b) = api.call("POST", "/v1/library/style/learn", Some(&t), Some(json!({ "provider": "openrouter" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    assert_eq!(b["guide"], GUIDE);
    {
        let calls = seen.0.lock().unwrap();
        let user = calls[0]["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("# Sample 1 (ACME-OQ-112.txt)") && user.contains("Expected result: The home page is displayed."));
        assert!(calls[0]["messages"][0]["content"].as_str().unwrap().contains("Never copy facts from the samples"));
    }
    let (_, style) = api.call("GET", "/v1/library/style", Some(&member), None).await;
    assert_eq!((style["guide"]["source"].as_str(), style["guide"]["model"].as_str()), (Some("ai"), Some("test/model")));
    assert_eq!(style["samples"].as_array().unwrap().len(), 1);

    // Drafts follow it.
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "LIMS IV", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    api.call("POST", &format!("/v1/sessions/{sid}/documents/testing/draft"), Some(&t), Some(json!({ "provider": "openrouter" }))).await;
    for _ in 0..200 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/testing"), Some(&t), None).await;
        if d["document"]["status"] != "drafting" {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    {
        let calls = seen.0.lock().unwrap();
        let user = calls[1]["messages"][1]["content"].as_str().unwrap();
        assert!(user.contains("# Company writing style") && user.contains("Short, impersonal sentences"), "{user}");
    }

    // Edited by hand; then cleared.
    let (s, _) = api.call("PUT", "/v1/library/style", Some(&member), Some(json!({ "guide": "x" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    api.call("PUT", "/v1/library/style", Some(&t), Some(json!({ "guide": "- Use British spelling." }))).await;
    let (_, style) = api.call("GET", "/v1/library/style", Some(&t), None).await;
    assert_eq!((style["guide"]["source"].as_str(), style["guide"]["guide"].as_str()), (Some("person"), Some("- Use British spelling.")));
    api.call("PUT", "/v1/library/style", Some(&t), Some(json!({ "guide": "  " }))).await;
    let (_, style) = api.call("GET", "/v1/library/style", Some(&t), None).await;
    assert!(style["guide"].is_null());

    assert_eq!(api.call("DELETE", &format!("/v1/library/style/samples/{sample_id}"), Some(&t), None).await.0, StatusCode::NO_CONTENT);
    let (_, log) = api.call("GET", "/v1/audit", Some(&t), None).await;
    let text = log.to_string();
    for a in ["style.sample_added", "style.learnt", "style.edited", "style.cleared", "style.sample_removed"] {
        assert!(text.contains(a), "missing {a}");
    }
    // The learning call is in the usage record too.
    let (_, u) = api.call("GET", "/v1/workspace/ai/usage", Some(&t), None).await;
    assert_eq!(u["totals"][0]["calls"], 2);
}
