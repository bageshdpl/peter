//! M3 guided steps and M4 AI drafting. OpenRouter and Gemini are stood in for
//! by local servers that speak their formats, so no key or network is needed.

mod common;

use std::{
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::post,
};
use common::{Api, token};
use peter_server::state::{AppState, Config};
use serde_json::{Value, json};

/// Builds an Api whose AI settings are adjusted before the server starts.
async fn api_with(adjust: impl FnOnce(&mut Config)) -> Api {
    let mut config = Config::for_tests();
    adjust(&mut config);
    let mail = Arc::new(common::CaptureMailer::default());
    let state = AppState::connect(config, mail.clone()).await.unwrap();
    Api { app: peter_server::app(state), mail }
}

/// A session filed in a project, with three observed steps and one text source.
async fn session_with_evidence(api: &Api) -> (String, String) {
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, p) = api.call("POST", "/v1/projects", Some(&t),
        Some(json!({ "name": "LIMS", "system_name": "FakeLIMS 8.2", "description": "Sample login and release" }))).await;
    let pid = p["id"].as_str().unwrap().to_string();
    api.upload(&format!("/v1/projects/{pid}/sources"), &t, "SOP-QA-014.txt", b"Samples must be logged in within 24 hours of receipt.").await;
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t),
        Some(json!({ "title": "LIMS IV", "validation_type": "initial", "project_id": pid }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    let obs = format!("/v1/sessions/{sid}/observations");
    for step in [
        json!({ "kind": "navigate", "url": "https://lims/samples", "page_title": "Samples" }),
        json!({ "kind": "click", "url": "https://lims/samples", "page_title": "Samples", "target_label": "Log in sample" }),
        json!({ "kind": "input", "url": "https://lims/login", "target_label": "Password", "value": "•••• (hidden)", "masked": true }),
    ] {
        let (s, b) = api.call("POST", &obs, Some(&t), Some(step)).await;
        assert_eq!(s, StatusCode::CREATED, "{b}");
    }
    (t, sid)
}

/// Starts a draft and waits for it to finish.
async fn draft(api: &Api, t: &str, sid: &str, step: &str, provider: &str) -> Value {
    let (s, b) = api.call("POST", &format!("/v1/sessions/{sid}/documents/{step}/draft"), Some(t),
        Some(json!({ "provider": provider, "instructions": "Use US spelling." }))).await;
    assert_eq!(s, StatusCode::ACCEPTED, "{b}");
    wait_for(api, t, sid, step).await
}

async fn wait_for(api: &Api, t: &str, sid: &str, step: &str) -> Value {
    for _ in 0..100 {
        let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/{step}"), Some(t), None).await;
        if doc["document"]["status"] != "drafting" {
            return doc;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("draft did not finish");
}

#[tokio::test]
async fn guided_steps_track_status_and_current_step() {
    let api = Api::new().await;
    let (t, sid) = session_with_evidence(&api).await;

    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/current"), Some(&t), Some(json!({ "step_id": "requirements" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/steps/observe"), Some(&t), Some(json!({ "status": "reviewed" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/steps/specification"), Some(&t), Some(json!({ "status": "skipped" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);

    let (_, got) = api.call("GET", &format!("/v1/sessions/{sid}"), Some(&t), None).await;
    assert_eq!(got["session"]["current_step"], "requirements");
    let states: std::collections::BTreeMap<String, String> = got["step_states"].as_array().unwrap().iter()
        .map(|s| (s["step_id"].as_str().unwrap().to_string(), s["status"].as_str().unwrap().to_string()))
        .collect();
    assert_eq!(states["requirements"], "in_progress");
    assert_eq!(states["observe"], "reviewed");
    assert_eq!(states["specification"], "skipped");

    // Opening a reviewed step does not undo the review.
    api.call("POST", &format!("/v1/sessions/{sid}/current"), Some(&t), Some(json!({ "step_id": "observe" }))).await;
    let (_, got) = api.call("GET", &format!("/v1/sessions/{sid}"), Some(&t), None).await;
    let observe = got["step_states"].as_array().unwrap().iter().find(|s| s["step_id"] == "observe").unwrap().clone();
    assert_eq!(observe["status"], "reviewed");

    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/steps/observe"), Some(&t), Some(json!({ "status": "done" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/steps/nope"), Some(&t), Some(json!({ "status": "reviewed" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let viewer = token(&api.add_member(&t, "view@example.com", "viewer").await);
    let (s, _) = api.call("PUT", &format!("/v1/sessions/{sid}/steps/plan"), Some(&viewer), Some(json!({ "status": "reviewed" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    let (_, audit) = api.call("GET", "/v1/audit?limit=5", Some(&t), None).await;
    let changes: Vec<&Value> = audit["events"].as_array().unwrap().iter().filter(|e| e["action"] == "step.status_changed").collect();
    assert_eq!(changes[0]["detail"], json!({ "step_id": "specification", "from": "not_started", "to": "skipped" }));
}

#[tokio::test]
async fn sample_draft_then_edit_with_versions() {
    let api = Api::new().await;
    let (t, sid) = session_with_evidence(&api).await;

    let (_, providers) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    assert_eq!(providers["providers"].as_array().unwrap().iter().map(|p| p["id"].as_str().unwrap()).collect::<Vec<_>>(), ["sample"]);
    let (s, b) = api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&t),
        Some(json!({ "provider": "openrouter" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("unknown_provider")));

    let doc = draft(&api, &t, &sid, "observe", "sample").await;
    assert_eq!(doc["document"]["status"], "ready", "{doc}");
    let (_, session) = api.call("GET", &format!("/v1/sessions/{sid}"), Some(&t), None).await;
    assert_eq!(session["step_states"][0]["status"], "in_progress", "drafting starts the step");
    assert_eq!(doc["title"], "Observation summary");
    let current = &doc["current"];
    assert_eq!((current["version"].as_i64(), current["origin"].as_str()), (Some(1), Some("ai")));
    let body = current["body"].as_str().unwrap();
    assert!(body.contains("[O2]") && body.contains("[S1]"), "{body}");
    assert!(!body.contains("hidden value") || !body.contains("Sup3r"), "secret leaked");
    assert_eq!(current["citations"]["sources"]["S1"]["filename"], "SOP-QA-014.txt");
    assert_eq!(current["observations"]["O2"]["label"], "Log in sample");
    assert_eq!(current["citations"]["unknown"], json!([]));

    // A person's edit becomes version 2; an edit from a stale base is refused.
    let path = format!("/v1/sessions/{sid}/documents/observe");
    let (s, saved) = api.call("PUT", &path, Some(&t),
        Some(json!({ "body": "# Observation summary\n\nSamples were logged in [O2]. Backup [GAP: no SOP]. Bad [O77].", "base_version": 1 }))).await;
    assert_eq!(s, StatusCode::OK, "{saved}");
    assert_eq!((saved["current"]["version"].as_i64(), saved["current"]["origin"].as_str()), (Some(2), Some("person")));
    assert_eq!(saved["current"]["citations"]["unknown"], json!(["O77"]));
    assert_eq!(saved["current"]["citations"]["gaps"], 1);
    assert_eq!(saved["versions"].as_array().unwrap().len(), 2);
    let (s, b) = api.call("PUT", &path, Some(&t), Some(json!({ "body": "x", "base_version": 1 }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("stale")));
    let (_, v1) = api.call("GET", &format!("{path}/versions/1"), Some(&t), None).await;
    assert_eq!(v1["origin"], "ai");

    let (_, session) = api.call("GET", &format!("/v1/sessions/{sid}"), Some(&t), None).await;
    assert_eq!(session["documents"][0]["current_version"], 2);
    let (_, audit) = api.call("GET", "/v1/audit?limit=3", Some(&t), None).await;
    assert_eq!(audit["events"][0]["action"], "document.edited");
    assert_eq!(audit["events"][1]["action"], "document.drafted");
}

#[derive(Clone, Default)]
struct Seen(Arc<Mutex<Vec<(HeaderMap, String, Value)>>>);

async fn fake_provider(router: Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    format!("http://{addr}")
}

#[tokio::test]
async fn openrouter_draft_sends_the_evidence_and_checks_citations() {
    let seen = Seen::default();
    let router = Router::new()
        .route("/chat/completions", post(|State(seen): State<Seen>, headers: HeaderMap, Json(body): Json<Value>| async move {
            seen.0.lock().unwrap().push((headers, "/chat/completions".into(), body));
            Json(json!({
                "model": "test/model-1",
                "choices": [{ "message": { "role": "assistant",
                    "content": "```markdown\n# User requirements\n\n| ID | Requirement | Evidence |\n| --- | --- | --- |\n| URS-001 | The system shall log in samples. | [O2][S1] |\n\nBuilds on [D:observe]. Invented [O99]. [GAP: audit trail not observed]\n```" } }],
                "usage": { "prompt_tokens": 1234, "completion_tokens": 321 }
            }))
        }))
        .with_state(seen.clone());
    let base = fake_provider(router).await;
    let api = api_with(|c| {
        c.ai.openrouter_key = Some("or-test-key".into());
        c.ai.openrouter_base = base.clone();
        c.ai.openrouter_model = "test/model-1".into();
    })
    .await;
    let (t, sid) = session_with_evidence(&api).await;
    let (_, providers) = api.call("GET", "/v1/ai/providers", Some(&t), None).await;
    assert_eq!(providers["providers"][0]["id"], "openrouter");
    assert!(!providers.to_string().contains("or-test-key"), "key exposed");

    // An earlier document for the prompt to build on.
    draft(&api, &t, &sid, "observe", "sample").await;
    let doc = draft(&api, &t, &sid, "requirements", "openrouter").await;
    assert_eq!(doc["document"]["status"], "ready", "{doc}");
    let current = &doc["current"];
    assert_eq!(current["provider"], "openrouter");
    assert_eq!(current["model"], "test/model-1");
    assert_eq!((current["prompt_tokens"].as_i64(), current["completion_tokens"].as_i64()), (Some(1234), Some(321)));
    assert!(current["body"].as_str().unwrap().starts_with("# User requirements"), "fence not removed");
    assert_eq!(current["citations"]["cited"], json!(["D:observe", "O2", "S1"]));
    assert_eq!(current["citations"]["unknown"], json!(["O99"]));
    assert_eq!(current["citations"]["gaps"], 1);

    // What reached the provider.
    let calls = seen.0.lock().unwrap();
    let (headers, _, body) = &calls[0];
    assert_eq!(headers["authorization"], "Bearer or-test-key");
    assert_eq!(body["model"], "test/model-1");
    let system = body["messages"][0]["content"].as_str().unwrap();
    let user = body["messages"][1]["content"].as_str().unwrap();
    assert!(system.contains("Cite the evidence for every factual statement"));
    assert!(user.contains("Write the User requirements and risk assessment"));
    assert!(user.contains("[O2] clicked \"Log in sample\""));
    assert!(user.contains("entered a hidden value in \"Password\""));
    assert!(user.contains("### [S1] SOP-QA-014.txt\nSamples must be logged in within 24 hours"));
    assert!(user.contains("### [D:observe] Observation summary"));
    assert!(user.contains("Use US spelling."));
    assert!(user.contains("System and version: FakeLIMS 8.2"));
}

#[tokio::test]
async fn gemini_draft_failure_and_busy_guard() {
    let seen = Seen::default();
    let router = Router::new()
        .route("/models/{call}", post(|State(seen): State<Seen>, Path(call): Path<String>, headers: HeaderMap, Json(body): Json<Value>| async move {
            let n = { let mut s = seen.0.lock().unwrap(); s.push((headers, call, body)); s.len() };
            tokio::time::sleep(Duration::from_millis(400)).await;
            if n == 1 {
                (StatusCode::TOO_MANY_REQUESTS, Json(json!({ "error": { "code": 429, "message": "Quota exceeded for this project." } })))
            } else {
                (StatusCode::OK, Json(json!({
                    "candidates": [{ "content": { "role": "model", "parts": [{ "text": "# System assessment\n\nGxP impact: direct " }, { "text": "[O2]." }] } }],
                    "usageMetadata": { "promptTokenCount": 900, "candidatesTokenCount": 80 },
                    "modelVersion": "gemini-test-001"
                })))
            }
        }))
        .with_state(seen.clone());
    let base = fake_provider(router).await;
    let api = api_with(|c| {
        c.ai.gemini_key = Some("gm-test-key".into());
        c.ai.gemini_base = base.clone();
        c.ai.gemini_model = "gemini-test".into();
    })
    .await;
    let (t, sid) = session_with_evidence(&api).await;
    let path = format!("/v1/sessions/{sid}/documents/assessment/draft");

    // While a draft runs, a second one is refused.
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "provider": "gemini" }))).await;
    assert_eq!(s, StatusCode::ACCEPTED);
    let (s, b) = api.call("POST", &path, Some(&t), Some(json!({ "provider": "gemini" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("drafting")));

    // The first call fails: the provider's reason is kept, and nothing is versioned.
    let failed = wait_for(&api, &t, &sid, "assessment").await;
    assert_eq!(failed["document"]["status"], "failed");
    assert!(failed["document"]["error"].as_str().unwrap().contains("Quota exceeded for this project."), "{failed}");
    assert!(failed["current"].is_null());
    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&t), None).await;
    assert_eq!(audit["events"][0]["action"], "document.draft_failed");

    // Drafting again succeeds.
    let doc = draft(&api, &t, &sid, "assessment", "gemini").await;
    assert_eq!(doc["document"]["status"], "ready");
    assert_eq!(doc["current"]["body"], "# System assessment\n\nGxP impact: direct [O2].");
    assert_eq!(doc["current"]["model"], "gemini-test-001");
    assert_eq!(doc["current"]["prompt_tokens"], 900);

    let calls = seen.0.lock().unwrap();
    let (headers, call, body) = &calls[1];
    assert_eq!(call, "gemini-test:generateContent");
    assert_eq!(headers["x-goog-api-key"], "gm-test-key");
    assert!(body["systemInstruction"]["parts"][0]["text"].as_str().unwrap().starts_with("You are Peter"));
    assert!(body["contents"][0]["parts"][0]["text"].as_str().unwrap().contains("Write the System assessment"));
}

#[tokio::test]
async fn documents_respect_workspaces_and_archiving() {
    let api = Api::new().await;
    let (t, sid) = session_with_evidence(&api).await;
    let other = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("GET", &format!("/v1/sessions/{sid}/documents/observe"), Some(&other), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&other),
        Some(json!({ "provider": "sample" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (_, empty) = api.call("GET", &format!("/v1/sessions/{sid}/documents/plan"), Some(&t), None).await;
    assert!(empty["document"].is_null());
    assert_eq!((empty["title"].as_str(), empty["draftable"].as_bool()), (Some("Validation plan"), Some(true)));

    api.call("POST", &format!("/v1/sessions/{sid}/archive"), Some(&t), None).await;
    let (s, _) = api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&t),
        Some(json!({ "provider": "sample" }))).await;
    assert_eq!(s, StatusCode::CONFLICT);
}

#[tokio::test]
async fn change_validation_drafts_cite_the_earlier_validation() {
    let seen = Seen::default();
    let router = Router::new()
        .route("/chat/completions", post(|State(seen): State<Seen>, headers: HeaderMap, Json(body): Json<Value>| async move {
            seen.0.lock().unwrap().push((headers, "/chat/completions".into(), body));
            Json(json!({
                "model": "test/model-1",
                "choices": [{ "message": { "role": "assistant",
                    "content": "# Impact assessment\n\nThe login screen changes [O1]; baseline test [B:observe]. Unknown [B:testing]." } }],
                "usage": { "prompt_tokens": 10, "completion_tokens": 5 }
            }))
        }))
        .with_state(seen.clone());
    let base = fake_provider(router).await;
    let api = api_with(|c| {
        c.ai.openrouter_key = Some("or-test-key".into());
        c.ai.openrouter_base = base.clone();
        c.ai.openrouter_model = "test/model-1".into();
    })
    .await;
    let (t, iv) = session_with_evidence(&api).await;
    draft(&api, &t, &iv, "observe", "sample").await;
    let (_, ivs) = api.call("GET", &format!("/v1/sessions/{iv}"), Some(&t), None).await;
    let pid = ivs["session"]["project_id"].as_str().unwrap();

    let (s, c) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({
        "title": "LIMS 8.3", "validation_type": "change", "project_id": pid, "baseline_session_id": iv }))).await;
    assert_eq!(s, StatusCode::CREATED, "{c}");
    let cid = c["session"]["id"].as_str().unwrap().to_string();
    let (s, _) = api.call("POST", &format!("/v1/sessions/{cid}/observations"), Some(&t),
        Some(json!({ "kind": "navigate", "url": "https://lims/login", "page_title": "Login" }))).await;
    assert_eq!(s, StatusCode::CREATED);

    // A sample draft lists the earlier validation's documents.
    let doc = draft(&api, &t, &cid, "baseline", "sample").await;
    let body = doc["current"]["body"].as_str().unwrap();
    assert!(body.contains("[B:observe]"), "{body}");
    assert_eq!(doc["current"]["citations"]["unknown"], json!([]));
    assert_eq!(doc["current"]["citations"]["baseline"]["title"], "LIMS IV");

    let doc = draft(&api, &t, &cid, "impact", "openrouter").await;
    assert_eq!(doc["title"], "Impact assessment");
    assert_eq!(doc["current"]["citations"]["cited"], json!(["B:observe", "O1"]));
    assert_eq!(doc["current"]["citations"]["unknown"], json!(["B:testing"]));
    let user = seen.0.lock().unwrap()[0].2["messages"][1]["content"].as_str().unwrap().to_string();
    assert!(user.contains("Write the Impact assessment for this change validation"));
    assert!(user.contains("## Earlier validation this session builds on\nInitial validation \"LIMS IV\""), "{user}");
    assert!(user.contains("### [B:observe] Observation summary"));
    assert!(user.contains("### [D:baseline] Baseline documents"));

    // The Word export handles [B:step] citations.
    let (s, _, bytes) = api.raw_get(&format!("/v1/sessions/{cid}/documents/impact/export/docx"), &t).await;
    assert_eq!(s, StatusCode::OK);
    assert!(bytes.starts_with(b"PK"));
}
