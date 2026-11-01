use std::sync::{Arc, Mutex};

use axum::{
    Router,
    body::Body,
    http::{Request, StatusCode, header},
};
use http_body_util::BodyExt;
use peter_server::state::{AppState, Config, Mailer};
use serde_json::{Value, json};
use tower::ServiceExt;

#[derive(Default)]
struct CaptureMailer(Mutex<Vec<(String, String)>>);

impl Mailer for CaptureMailer {
    fn send_link(&self, to: &str, _subject: &str, link: &str) {
        self.0.lock().unwrap().push((to.to_string(), link.to_string()));
    }
}

impl CaptureMailer {
    fn last_token_for(&self, to: &str) -> String {
        let sent = self.0.lock().unwrap();
        let (_, link) = sent.iter().rev().find(|(t, _)| t == to).expect("no email sent");
        link.split("#token=").nth(1).unwrap().to_string()
    }
}

struct Api {
    app: Router,
    mail: Arc<CaptureMailer>,
}

impl Api {
    async fn new() -> Self {
        let mail = Arc::new(CaptureMailer::default());
        let state = AppState::connect(Config::for_tests(), mail.clone()).await.unwrap();
        Self { app: peter_server::app(state), mail }
    }

    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
        let mut req = Request::builder().method(method).uri(path);
        if let Some(t) = token {
            req = req.header(header::AUTHORIZATION, format!("Bearer {t}"));
        }
        let req = match body {
            Some(b) => req.header(header::CONTENT_TYPE, "application/json").body(Body::from(b.to_string())),
            None => req.body(Body::empty()),
        }
        .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        let value = if bytes.is_empty() { Value::Null } else { serde_json::from_slice(&bytes).unwrap() };
        (status, value)
    }

    async fn signup(&self, email: &str, workspace: &str) -> Value {
        let (status, body) = self
            .call("POST", "/v1/auth/signup", None, Some(json!({
                "email": email, "password": "correct-horse-1", "name": "Test User", "workspace_name": workspace
            })))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// An owner invites `email` with `role`; the invitee creates an account.
    async fn add_member(&self, owner_token: &str, email: &str, role: &str) -> Value {
        let (status, body) =
            self.call("POST", "/v1/members", Some(owner_token), Some(json!({ "email": email, "role": role }))).await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        let token = self.mail.last_token_for(email);
        let (status, body) = self
            .call("POST", "/v1/auth/accept-invite", None, Some(json!({
                "token": token, "password": "another-pass-1", "name": "Invitee"
            })))
            .await;
        assert_eq!(status, StatusCode::OK, "{body}");
        body
    }
}

fn token(session: &Value) -> String {
    session["access_token"].as_str().unwrap().to_string()
}

#[tokio::test]
async fn signup_makes_an_owner_and_me_works() {
    let api = Api::new().await;
    let s = api.signup("Ana@Example.com", "Acme Pharma").await;
    assert_eq!(s["user"]["email"], "ana@example.com");
    assert_eq!(s["workspace"]["role"], "owner");

    let (status, me) = api.call("GET", "/v1/auth/me", Some(&token(&s)), None).await;
    assert_eq!(status, StatusCode::OK);
    assert_eq!(me["workspace"]["name"], "Acme Pharma");
}

#[tokio::test]
async fn signup_rejects_duplicates_weak_passwords_and_bad_emails() {
    let api = Api::new().await;
    api.signup("ana@example.com", "Acme").await;
    let body = |email: &str, pw: &str| json!({ "email": email, "password": pw, "name": "A", "workspace_name": "W" });

    let (s, b) = api.call("POST", "/v1/auth/signup", None, Some(body("ANA@example.com", "long-enough-1"))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("email_taken")));
    let (s, b) = api.call("POST", "/v1/auth/signup", None, Some(body("bo@example.com", "short"))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("weak_password")));
    let (s, b) = api.call("POST", "/v1/auth/signup", None, Some(body("not-an-email", "long-enough-1"))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_email")));
    let (s, b) = api.call("POST", "/v1/auth/signup", None, Some(json!({ "email": "x" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_body")));
}

#[tokio::test]
async fn login_logout_and_refresh_rotation() {
    let api = Api::new().await;
    api.signup("ana@example.com", "Acme").await;

    let (s, _) = api.call("POST", "/v1/auth/login", None,
        Some(json!({ "email": "ana@example.com", "password": "wrong-password" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = api.call("POST", "/v1/auth/login", None,
        Some(json!({ "email": "nobody@example.com", "password": "whatever-123" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, login) = api.call("POST", "/v1/auth/login", None,
        Some(json!({ "email": "ana@example.com", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::OK);

    // Refreshing issues new tokens and spends the old ones.
    let old_refresh = login["refresh_token"].as_str().unwrap();
    let (s, refreshed) = api.call("POST", "/v1/auth/refresh", None, Some(json!({ "refresh_token": old_refresh }))).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", "/v1/auth/refresh", None, Some(json!({ "refresh_token": old_refresh }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&token(&login)), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let new_access = token(&refreshed);
    let (s, _) = api.call("POST", "/v1/auth/logout", Some(&new_access), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&new_access), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
}

#[tokio::test]
async fn invite_flow_for_new_and_existing_accounts() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);

    let joined = api.add_member(&owner, "bo@example.com", "member").await;
    assert_eq!(joined["workspace"]["role"], "member");

    // An existing account accepts with its own password and gains a second workspace.
    let other_owner = token(&api.signup("cy@example.com", "Beta Labs").await);
    let (s, _) = api.call("POST", "/v1/members", Some(&other_owner),
        Some(json!({ "email": "ana@example.com", "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let invite = api.mail.last_token_for("ana@example.com");
    let (s, _) = api.call("POST", "/v1/auth/accept-invite", None,
        Some(json!({ "token": invite, "password": "wrong-password" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);
    let (s, accepted) = api.call("POST", "/v1/auth/accept-invite", None,
        Some(json!({ "token": invite, "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(accepted["workspaces"].as_array().unwrap().len(), 2);

    // An invite works once.
    let (s, _) = api.call("POST", "/v1/auth/accept-invite", None,
        Some(json!({ "token": invite, "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    // Inviting an existing member is refused.
    let (s, b) = api.call("POST", "/v1/members", Some(&owner),
        Some(json!({ "email": "bo@example.com", "role": "viewer" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("already_member")));
}

#[tokio::test]
async fn revoked_invites_cannot_be_used() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let (_, created) = api.call("POST", "/v1/members", Some(&owner),
        Some(json!({ "email": "bo@example.com", "role": "member" }))).await;
    let invite_token = api.mail.last_token_for("bo@example.com");
    let id = created["invite"]["id"].as_str().unwrap();
    let (s, _) = api.call("DELETE", &format!("/v1/invites/{id}"), Some(&owner), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("POST", "/v1/auth/accept-invite", None,
        Some(json!({ "token": invite_token, "password": "another-pass-1", "name": "Bo" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn role_rules() {
    let api = Api::new().await;
    let owner_session = api.signup("ana@example.com", "Acme").await;
    let owner = token(&owner_session);
    let owner_id = owner_session["user"]["id"].as_str().unwrap().to_string();
    let admin_session = api.add_member(&owner, "admin@example.com", "admin").await;
    let admin = token(&admin_session);
    let member_session = api.add_member(&owner, "mem@example.com", "member").await;
    let member = token(&member_session);
    let member_id = member_session["user"]["id"].as_str().unwrap().to_string();

    // Members cannot invite; admins cannot create owners or touch owners.
    let (s, _) = api.call("POST", "/v1/members", Some(&member), Some(json!({ "email": "x@example.com", "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("POST", "/v1/members", Some(&admin), Some(json!({ "email": "x@example.com", "role": "owner" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("PATCH", &format!("/v1/members/{owner_id}"), Some(&admin), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (s, _) = api.call("DELETE", &format!("/v1/members/{owner_id}"), Some(&admin), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // Admins can change and remove ordinary members.
    let (s, _) = api.call("PATCH", &format!("/v1/members/{member_id}"), Some(&admin), Some(json!({ "role": "viewer" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, me) = api.call("GET", "/v1/auth/me", Some(&member), None).await;
    assert_eq!((s, me["workspace"]["role"].as_str()), (StatusCode::OK, Some("viewer")));

    // The last owner cannot demote themselves or leave.
    let (s, b) = api.call("PATCH", &format!("/v1/members/{owner_id}"), Some(&owner), Some(json!({ "role": "admin" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("last_owner")));
    let (s, _) = api.call("DELETE", &format!("/v1/members/{owner_id}"), Some(&owner), None).await;
    assert_eq!(s, StatusCode::CONFLICT);

    // A removed member loses access at once.
    let (s, _) = api.call("DELETE", &format!("/v1/members/{member_id}"), Some(&admin), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&member), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Only admins see the audit trail.
    let viewer = token(&api.add_member(&owner, "view@example.com", "viewer").await);
    let (s, _) = api.call("GET", "/v1/audit", Some(&viewer), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn workspaces_create_switch_and_rename() {
    let api = Api::new().await;
    let first = token(&api.signup("ana@example.com", "Acme").await);
    let (s, created) = api.call("POST", "/v1/workspaces", Some(&first), Some(json!({ "name": "Second Site" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let second_id = created["id"].as_str().unwrap();

    let (s, switched) = api.call("POST", "/v1/auth/switch", Some(&first), Some(json!({ "workspace_id": second_id }))).await;
    assert_eq!(s, StatusCode::OK);
    assert_eq!(switched["workspace"]["name"], "Second Site");
    // Switching ends the old session.
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&first), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let second = token(&switched);
    let (s, _) = api.call("PATCH", "/v1/workspace", Some(&second), Some(json!({ "name": "Site B" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (_, ws) = api.call("GET", "/v1/workspace", Some(&second), None).await;
    assert_eq!(ws["name"], "Site B");

    // Cannot switch into a workspace you are not in.
    let outsider = token(&api.signup("zed@example.com", "Other").await);
    let (s, _) = api.call("POST", "/v1/auth/switch", Some(&outsider), Some(json!({ "workspace_id": second_id }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
}

#[tokio::test]
async fn password_reset_and_change() {
    let api = Api::new().await;
    let session = token(&api.signup("ana@example.com", "Acme").await);

    // Unknown emails get the same answer and no email.
    let (s, _) = api.call("POST", "/v1/auth/reset/request", None, Some(json!({ "email": "nobody@example.com" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    assert!(api.mail.0.lock().unwrap().is_empty());

    let (s, _) = api.call("POST", "/v1/auth/reset/request", None, Some(json!({ "email": "ana@example.com" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let reset = api.mail.last_token_for("ana@example.com");
    let (s, _) = api.call("POST", "/v1/auth/reset/confirm", None, Some(json!({ "token": reset, "password": "brand-new-pass-1" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    // The link works once, and existing sessions end.
    let (s, _) = api.call("POST", "/v1/auth/reset/confirm", None, Some(json!({ "token": reset, "password": "other-pass-123" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&session), None).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    let (s, login) = api.call("POST", "/v1/auth/login", None,
        Some(json!({ "email": "ana@example.com", "password": "brand-new-pass-1" }))).await;
    assert_eq!(s, StatusCode::OK);
    let current = token(&login);
    let (s, _) = api.call("POST", "/v1/auth/change-password", Some(&current),
        Some(json!({ "current_password": "wrong-password", "new_password": "x-another-pass-1" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
    let (s, _) = api.call("POST", "/v1/auth/change-password", Some(&current),
        Some(json!({ "current_password": "brand-new-pass-1", "new_password": "x-another-pass-1" }))).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("GET", "/v1/auth/me", Some(&current), None).await;
    assert_eq!(s, StatusCode::OK);
}

#[tokio::test]
async fn audit_trail_records_actions_and_pages() {
    let api = Api::new().await;
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    api.add_member(&owner, "bo@example.com", "member").await;

    let (s, page) = api.call("GET", "/v1/audit?limit=2", Some(&owner), None).await;
    assert_eq!(s, StatusCode::OK);
    let actions: Vec<&str> = page["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["member.joined", "member.invited"]);
    let before = page["next_before"].as_i64().unwrap();
    let (_, rest) = api.call("GET", &format!("/v1/audit?before={before}"), Some(&owner), None).await;
    let actions: Vec<&str> = rest["events"].as_array().unwrap().iter().map(|e| e["action"].as_str().unwrap()).collect();
    assert_eq!(actions, ["workspace.created"]);
    assert!(rest["next_before"].is_null());
}
