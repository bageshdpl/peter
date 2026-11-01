//! Sign-in limits, two-factor sign-in, the workspace requirement, and active sessions.

mod common;

use axum::{
    body::Body,
    http::{Request, StatusCode, header},
};
use common::{Api, token};
use http_body_util::BodyExt;
use peter_server::mfa;
use serde_json::{Value, json};
use tower::ServiceExt;

fn now() -> i64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs() as i64
}

/// The code an authenticator app would show, `offset` steps from now.
fn app_code(secret: &str, offset: i64) -> String {
    mfa::code_at(&mfa::base32_decode(secret).unwrap(), now() / mfa::STEP_SECS + offset, mfa::DIGITS)
}

async fn login(api: &Api, email: &str, password: &str) -> (StatusCode, Value) {
    api.call("POST", "/v1/auth/login", None, Some(json!({ "email": email, "password": password }))).await
}

/// Turns on two-factor sign-in for the signed-in person; returns the secret and recovery codes.
async fn enable_mfa(api: &Api, t: &str) -> (String, Vec<String>) {
    let (s, setup) = api.call("POST", "/v1/auth/mfa/setup", Some(t), None).await;
    assert_eq!(s, StatusCode::OK, "{setup}");
    let secret = setup["secret"].as_str().unwrap().to_string();
    assert!(setup["otpauth_url"].as_str().unwrap().contains(&secret));
    let (s, enabled) = api.call("POST", "/v1/auth/mfa/enable", Some(t), Some(json!({ "code": app_code(&secret, 0) }))).await;
    assert_eq!(s, StatusCode::OK, "{enabled}");
    let codes = enabled["recovery_codes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().to_string()).collect();
    (secret, codes)
}

#[tokio::test]
async fn wrong_passwords_lock_the_account_for_a_while() {
    let api = Api::new().await;
    api.signup("ana@example.com", "Acme").await;
    for i in 1..=4 {
        let (s, b) = login(&api, "ana@example.com", "wrong-password").await;
        assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::UNAUTHORIZED, Some("unauthorized")), "attempt {i}");
    }
    let (s, b) = login(&api, "ana@example.com", "wrong-password").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::TOO_MANY_REQUESTS, Some("locked")));
    assert!(b["error"]["message"].as_str().unwrap().contains("15 minutes"));
    // Even the right password is refused while locked.
    let (s, _) = login(&api, "ana@example.com", "correct-horse-1").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    // Unknown emails lock the same way, so a lock does not reveal an account.
    for _ in 0..4 {
        login(&api, "nobody@example.com", "whatever-123").await;
    }
    let (s, _) = login(&api, "nobody@example.com", "whatever-123").await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);

    // Resetting the password by email ends the lock.
    api.call("POST", "/v1/auth/reset/request", None, Some(json!({ "email": "ana@example.com" }))).await;
    let reset = api.mail.last_token_for("ana@example.com");
    let (s, _) = api.call("POST", "/v1/auth/reset/confirm", None, Some(json!({ "token": reset, "password": "brand-new-pass-1" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = login(&api, "ana@example.com", "brand-new-pass-1").await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn a_correct_login_resets_the_count() {
    let api = Api::new().await;
    api.signup("ana@example.com", "Acme").await;
    for _ in 0..4 {
        login(&api, "ana@example.com", "wrong-password").await;
    }
    assert_eq!(login(&api, "ana@example.com", "correct-horse-1").await.0, StatusCode::OK);
    for _ in 0..4 {
        assert_eq!(login(&api, "ana@example.com", "wrong-password").await.0, StatusCode::UNAUTHORIZED);
    }
}

#[tokio::test]
async fn two_factor_setup_and_sign_in() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);

    let (_, before) = api.call("GET", "/v1/auth/mfa", Some(&t), None).await;
    assert_eq!(before["enabled"], false);
    // Setup is not active until a code confirms it.
    let (_, setup) = api.call("POST", "/v1/auth/mfa/setup", Some(&t), None).await;
    let (s, b) = api.call("POST", "/v1/auth/mfa/enable", Some(&t), Some(json!({ "code": "000000" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("wrong_code")));
    assert_eq!(login(&api, "ana@example.com", "correct-horse-1").await.1["mfa_required"], Value::Null);
    let secret = setup["secret"].as_str().unwrap();
    let enable_code = app_code(secret, 0);
    let (s, enabled) = api.call("POST", "/v1/auth/mfa/enable", Some(&t), Some(json!({ "code": enable_code }))).await;
    assert_eq!(s, StatusCode::OK);
    let codes: Vec<String> = enabled["recovery_codes"].as_array().unwrap().iter().map(|c| c.as_str().unwrap().into()).collect();
    assert_eq!(codes.len(), 10);
    let (_, after) = api.call("GET", "/v1/auth/mfa", Some(&t), None).await;
    assert_eq!((after["enabled"].as_bool(), after["recovery_codes_left"].as_i64()), (Some(true), Some(10)));

    // The password alone now gives only a ticket for the code step.
    let (s, step1) = login(&api, "ana@example.com", "correct-horse-1").await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(step1["mfa_required"], true);
    assert!(step1.get("access_token").is_none());
    let ticket = step1["mfa_token"].as_str().unwrap();
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": "123456" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    // The code used to enable is spent; the next one works (apps allow one step of drift).
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": enable_code }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED, "a code cannot be used twice");
    let next = app_code(secret, 1);
    let (s, session) = api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": next }))).await;
    assert_eq!(s, StatusCode::OK, "{session}");
    assert!(session["access_token"].is_string());
    // A ticket works once.
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": app_code(secret, 1) }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // A recovery code works once, in any case and with or without dashes.
    let (_, step1) = login(&api, "ana@example.com", "correct-horse-1").await;
    let messy = codes[0].to_lowercase().replace('-', " ");
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None,
        Some(json!({ "mfa_token": step1["mfa_token"], "recovery_code": messy }))).await;
    assert_eq!(s, StatusCode::OK);
    let (_, step1) = login(&api, "ana@example.com", "correct-horse-1").await;
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None,
        Some(json!({ "mfa_token": step1["mfa_token"], "recovery_code": codes[0] }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, status) = api.call("GET", "/v1/auth/mfa", Some(&t), None).await;
    assert_eq!(status["recovery_codes_left"], 9);

    // Turning it off needs the password and a fresh code.
    let (s, _) = api.call("POST", "/v1/auth/mfa/disable", Some(&t), Some(json!({ "password": "nope-nope-1", "code": app_code(secret, 1) }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let fresh = mfa::code_at(&mfa::base32_decode(secret).unwrap(), now() / mfa::STEP_SECS - 1, mfa::DIGITS);
    let (s, _) = api.call("POST", "/v1/auth/mfa/disable", Some(&t), Some(json!({ "password": "correct-horse-1", "code": fresh }))).await;
    // Steps before the last used one are refused, so this is a wrong code.
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let (_, audit) = api.call("GET", "/v1/audit?limit=20", Some(&t), None).await;
    let logins: Vec<&Value> = audit["events"].as_array().unwrap().iter().filter(|e| e["action"] == "auth.login").collect();
    assert_eq!(logins[0]["detail"], json!({ "mfa": true, "method": "recovery_code" }));
    assert!(audit["events"].as_array().unwrap().iter().any(|e| e["action"] == "user.mfa_enabled"));
}

#[tokio::test]
async fn wrong_codes_count_towards_the_lock() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    enable_mfa(&api, &t).await;
    let (_, step1) = login(&api, "ana@example.com", "correct-horse-1").await;
    let ticket = step1["mfa_token"].clone();
    for _ in 0..4 {
        api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": "000000" }))).await;
    }
    let (s, _) = api.call("POST", "/v1/auth/login/mfa", None, Some(json!({ "mfa_token": ticket, "code": "000000" }))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    assert_eq!(login(&api, "ana@example.com", "correct-horse-1").await.0, StatusCode::TOO_MANY_REQUESTS);
}

#[tokio::test]
async fn workspace_can_require_two_factor() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let member = token(&api.add_member(&owner, "bo@example.com", "member").await);

    // An admin must have it before requiring it.
    let (s, b) = api.call("PUT", "/v1/workspace/security", Some(&owner), Some(json!({ "require_mfa": true }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("mfa_needed")));
    enable_mfa(&api, &owner).await;
    let (s, _) = api.call("PUT", "/v1/workspace/security", Some(&member), Some(json!({ "require_mfa": true }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, policy) = api.call("PUT", "/v1/workspace/security", Some(&owner), Some(json!({ "require_mfa": true }))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(policy["members_without_mfa"], 1);

    // Bo can sign in but do nothing except set it up.
    let (_, me) = api.call("GET", "/v1/auth/me", Some(&member), None).await;
    assert_eq!(me["mfa_setup_required"], true);
    let (s, b) = api.call("GET", "/v1/projects", Some(&member), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("mfa_setup_required")));
    let (_, fresh) = login(&api, "bo@example.com", "another-pass-1").await;
    assert_eq!(fresh["mfa_setup_required"], true);
    enable_mfa(&api, &member).await;
    let (s, _) = api.call("GET", "/v1/projects", Some(&member), None).await;
    assert_eq!(s, StatusCode::OK);
    let (_, ws) = api.call("GET", "/v1/workspace", Some(&owner), None).await;
    assert_eq!(ws["require_mfa"], true);
}

#[tokio::test]
async fn active_sessions_can_be_listed_and_signed_out() {
    let api = Api::new().await;
    let first = token(&api.signup("ana@example.com", "Acme").await);
    // A second device, with its own browser name.
    let req = Request::builder()
        .method("POST")
        .uri("/v1/auth/login")
        .header(header::CONTENT_TYPE, "application/json")
        .header(header::USER_AGENT, "Mozilla/5.0 (Windows NT 10.0) Chrome/130.0")
        .body(Body::from(json!({ "email": "ana@example.com", "password": "correct-horse-1" }).to_string()))
        .unwrap();
    let res = api.app.clone().oneshot(req).await.unwrap();
    let second: Value = serde_json::from_slice(&res.into_body().collect().await.unwrap().to_bytes()).unwrap();
    let second = second["access_token"].as_str().unwrap().to_string();

    let (_, list) = api.call("GET", "/v1/auth/sessions", Some(&second), None).await;
    let sessions = list["sessions"].as_array().unwrap();
    assert_eq!(sessions.len(), 2);
    let current = sessions.iter().find(|s| s["current"] == true).unwrap();
    assert!(current["user_agent"].as_str().unwrap().contains("Windows"));
    assert_eq!(current["workspace_name"], "Acme");
    let other_id = sessions.iter().find(|s| s["current"] == false).unwrap()["id"].as_str().unwrap().to_string();

    // Someone else cannot sign out Ana's sessions.
    let zed = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("DELETE", &format!("/v1/auth/sessions/{other_id}"), Some(&zed), None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);

    let (s, revoked) = api.call("POST", "/v1/auth/sessions/revoke-others", Some(&second), None).await;
    assert_eq!((s, revoked["revoked"].as_u64()), (StatusCode::OK, Some(1)));
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&first), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (_, list) = api.call("GET", "/v1/auth/sessions", Some(&second), None).await;
    assert_eq!(list["sessions"].as_array().unwrap().len(), 1);
}

#[tokio::test]
async fn signing_a_document_is_throttled_too() {
    let api = Api::new().await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, s) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "S", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap();
    api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&t), Some(json!({ "provider": "sample" }))).await;
    for _ in 0..50 {
        let (_, d) = api.call("GET", &format!("/v1/sessions/{sid}/documents/observe"), Some(&t), None).await;
        if d["document"]["status"] == "ready" {
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(30)).await;
    }
    let path = format!("/v1/sessions/{sid}/documents/observe/signatures");
    for _ in 0..4 {
        let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 1, "meaning": "reviewed", "password": "wrong-one-1" }))).await;
        assert_eq!(s, StatusCode::BAD_REQUEST);
    }
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 1, "meaning": "reviewed", "password": "wrong-one-1" }))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS);
    let (s, _) = api.call("POST", &path, Some(&t), Some(json!({ "version": 1, "meaning": "reviewed", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::TOO_MANY_REQUESTS, "locked even with the right password");
}
