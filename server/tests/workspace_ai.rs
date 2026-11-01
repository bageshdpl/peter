//! A workspace's own AI key, the shared key's monthly limit, and usage.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::{get, post},
};
use common::{Api, token};
use peter_server::state::{AppState, Config};
use serde_json::{Value, json};

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<String>>>);

async fn fake_openrouter() -> (String, Seen) {
    let seen = Seen::default();
    let router = Router::new()
        .route("/chat/completions", post(|State(seen): State<Seen>, headers: HeaderMap, Json(_): Json<Value>| async move {
            seen.0.lock().unwrap().push(headers["authorization"].to_str().unwrap().to_string());
            Json(json!({
                "model": "test/model-1",
                "choices": [{ "message": { "role": "assistant", "content": "# Requirements\n\nNothing observed yet. [GAP: record the system]" } }],
                "usage": { "prompt_tokens": 1200, "completion_tokens": 300, "cost": 0.0125 }
            }))
        }))
        .route("/key", get(|headers: HeaderMap| async move {
            if headers["authorization"] == "Bearer sk-or-workspace-key-9876" {
                (StatusCode::OK, Json(json!({ "data": { "usage": 1.5, "limit": 10.0 } })))
            } else {
                (StatusCode::UNAUTHORIZED, Json(json!({ "error": { "message": "No auth credentials found" } })))
            }
        }))
        .with_state(seen.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    (format!("http://{addr}"), seen)
}

async fn api_with(adjust: impl FnOnce(&mut Config)) -> Api {
    let mut config = Config::for_tests();
    adjust(&mut config);
    let mail = Arc::new(common::CaptureMailer::default());
    let state = AppState::connect(config, mail.clone()).await.unwrap();
    Api { app: peter_server::app(state), mail }
}

/// Starts a draft and waits for it; returns the start status and the final document.
async fn draft(api: &Api, t: &str, sid: &str, provider: &str) -> (StatusCode, Value) {
    let (s, b) = api.call("POST", &format!("/v1/sessions/{sid}/documents/requirements/draft"), Some(t), Some(json!({ "provider": provider }))).await;
    if s != StatusCode::ACCEPTED && s != StatusCode::CREATED && s != StatusCode::OK {
        return (s, b);
    }
    for _ in 0..200 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/requirements"), Some(t), None).await;
        if d["document"]["status"] != "drafting" {
            return (s, d);
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("draft did not finish");
}

#[tokio::test]
async fn workspace_keys_limits_and_usage() {
    let (base, seen) = fake_openrouter().await;
    let api = api_with(|c| {
        c.ai.openrouter_key = Some("server-key-0000".into());
        c.ai.openrouter_base = base.clone();
        c.ai.shared = true;
        c.ai.shared_monthly_tokens = Some(1000);
    })
    .await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let member = token(&api.add_member(&t, "bo@example.com", "member").await);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "LIMS IV", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();

    // On the server's key at first.
    let (_, p) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    assert_eq!((p["providers"][0]["id"].as_str(), p["providers"][0]["key_source"].as_str()), (Some("openrouter"), Some("server")));
    let (_, d) = draft(&api, &t, &sid, "openrouter").await;
    assert_eq!(d["document"]["status"], "ready", "{d}");
    assert_eq!(seen.0.lock().unwrap().last().unwrap(), "Bearer server-key-0000");

    // 1500 tokens used of the 1000 allowed: the shared key is refused now.
    let (_, ai) = api.call("GET", "/v1/workspace/ai", Some(&t), None).await;
    assert_eq!((ai["shared_tokens_this_month"].as_i64(), ai["shared_monthly_tokens"].as_i64()), (Some(1500), Some(1000)));
    let (s, b) = draft(&api, &t, &sid, "openrouter").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("ai_limit")));

    // Only admins set keys; keys must look like keys.
    let path = "/v1/workspace/ai/openrouter";
    assert_eq!(api.call("PUT", path, Some(&member), Some(json!({ "key": "sk-or-workspace-key-9876" }))).await.0, StatusCode::FORBIDDEN);
    let (s, b) = api.call("PUT", path, Some(&t), Some(json!({ "key": "short" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_key")));
    let (s, b) = api.call("PUT", path, Some(&t), Some(json!({ "model": "bad model!" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_model")));
    let (s, _) = api.call("PUT", "/v1/workspace/ai/anthropic", Some(&t), Some(json!({ "key": "sk-whatever-123456" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    // The workspace's own key: shown only by its last four characters.
    let (s, b) = api.call("PUT", path, Some(&t), Some(json!({ "key": "sk-or-workspace-key-9876", "model": "test/own-model" }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (_, ai) = api.call("GET", "/v1/workspace/ai", Some(&member), None).await;
    assert!(!ai.to_string().contains("sk-or-workspace"), "key exposed: {ai}");
    let or = &ai["providers"][0];
    assert_eq!((or["workspace_key"]["hint"].as_str(), or["in_use"].as_str(), or["workspace_key"]["model"].as_str()),
        (Some("…9876"), Some("workspace"), Some("test/own-model")));
    let (_, p) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    assert_eq!((p["providers"][0]["key_source"].as_str(), p["providers"][0]["model"].as_str()), (Some("workspace"), Some("test/own-model")));

    // Drafting now uses it, and the shared limit does not apply.
    let (_, d) = draft(&api, &member, &sid, "openrouter").await;
    assert_eq!(d["document"]["status"], "ready", "{d}");
    assert_eq!(seen.0.lock().unwrap().last().unwrap(), "Bearer sk-or-workspace-key-9876");

    // Checking a key costs nothing.
    let (_, c) = api.call("POST", "/v1/workspace/ai/openrouter/check", Some(&t), None).await;
    assert_eq!((c["ok"].as_bool(), c["key_source"].as_str()), (Some(true), Some("workspace")));
    assert!(c["message"].as_str().unwrap().contains("$1.50 of its $10.00"), "{c}");

    // Usage, for admins only.
    assert_eq!(api.call("GET", "/v1/workspace/ai/usage", Some(&member), None).await.0, StatusCode::FORBIDDEN);
    let (_, u) = api.call("GET", "/v1/workspace/ai/usage?days=7", Some(&t), None).await;
    let totals = u["totals"].as_array().unwrap();
    let by = |src: &str| totals.iter().find(|r| r["key_source"] == src).unwrap().clone();
    assert_eq!((by("server")["calls"].as_i64(), by("workspace")["calls"].as_i64()), (Some(1), Some(1)));
    assert_eq!(by("workspace")["prompt_tokens"], 1200);
    assert!((by("server")["cost_usd"].as_f64().unwrap() - 0.0125).abs() < 1e-9);
    assert_eq!(u["people"].as_array().unwrap().len(), 2);
    assert_eq!(u["recent"][0]["session_title"], "LIMS IV");

    // Removing the key falls back to the shared one, which is still over its limit.
    assert_eq!(api.call("DELETE", path, Some(&t), None).await.0, StatusCode::NO_CONTENT);
    let (s, _) = draft(&api, &t, &sid, "openrouter").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    let (_, log) = api.call("GET", "/v1/audit", Some(&t), None).await;
    let text = log.to_string();
    assert!(text.contains("ai.key_set") && text.contains("ai.key_removed"));
    assert!(!text.contains("sk-or-workspace-key"), "key in the audit trail");
}

#[tokio::test]
async fn without_sharing_only_a_workspace_key_drafts() {
    let (base, _) = fake_openrouter().await;
    let api = api_with(|c| {
        c.ai.openrouter_key = Some("server-key-0000".into());
        c.ai.openrouter_base = base.clone();
        c.ai.shared = false;
    })
    .await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, p) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    let ids: Vec<&str> = p["providers"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect();
    assert_eq!(ids, ["sample"]);
    let (_, ai) = api.call("GET", "/v1/workspace/ai", Some(&t), None).await;
    assert_eq!(ai["providers"][0]["server_key"], false);

    api.call("PUT", "/v1/workspace/ai/openrouter", Some(&t), Some(json!({ "key": "sk-or-other-key-1111" }))).await;
    let (_, c) = api.call("POST", "/v1/workspace/ai/openrouter/check", Some(&t), None).await;
    assert_eq!(c["ok"], false);
    assert!(c["message"].as_str().unwrap().contains("No auth credentials found"), "{c}");
    let (_, p) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    assert_eq!(p["providers"][0]["key_source"], "workspace");
}
