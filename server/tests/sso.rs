//! Single sign-on with OpenID Connect. The identity provider is stood in for
//! by a local server that signs real ID tokens with a test-only key.

mod common;

use std::sync::{Arc, Mutex};

use axum::{
    Form, Json, Router,
    extract::State,
    routing::{get, post},
};
use common::{Api, token};
use jsonwebtoken::{Algorithm, EncodingKey, Header};
use peter_server::state::{AppState, Config};
use axum::http::StatusCode;
use serde_json::{Value, json};

const KEY: &str = include_str!("fixtures/sso-test-key.pem");
const OTHER_KEY: &str = include_str!("fixtures/sso-other-key.pem");
const CLIENT_ID: &str = "peter-client";

/// What the stand-in provider will answer next, and what it last received.
#[derive(Clone, Default)]
struct Idp {
    base: Arc<Mutex<String>>,
    id_token: Arc<Mutex<String>>,
    received: Arc<Mutex<Vec<(String, String)>>>,
}

async fn idp() -> Idp {
    let idp = Idp::default();
    let router = Router::new()
        .route("/.well-known/openid-configuration", get(|State(idp): State<Idp>| async move {
            let base = idp.base.lock().unwrap().clone();
            Json(json!({ "issuer": base, "authorization_endpoint": format!("{base}/authorize"),
                "token_endpoint": format!("{base}/token"), "jwks_uri": format!("{base}/keys") }))
        }))
        .route("/keys", get(|| async {
            Json(json!({ "keys": [{ "kty": "RSA", "kid": "k1", "use": "sig", "alg": "RS256",
                "n": include_str!("fixtures/sso-test-key.n.txt"), "e": "AQAB" }] }))
        }))
        .route("/token", post(|State(idp): State<Idp>, Form(form): Form<Vec<(String, String)>>| async move {
            *idp.received.lock().unwrap() = form;
            Json(json!({ "access_token": "at", "token_type": "Bearer", "id_token": idp.id_token.lock().unwrap().clone() }))
        }))
        .with_state(idp.clone());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    *idp.base.lock().unwrap() = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, router).await.unwrap() });
    idp
}

impl Idp {
    fn issuer(&self) -> String {
        self.base.lock().unwrap().clone()
    }

    /// Makes the provider's next answer an ID token with these claims, signed with `key`.
    fn will_answer(&self, claims: Value, key: &str) {
        let mut header = Header::new(Algorithm::RS256);
        header.kid = Some("k1".into());
        *self.id_token.lock().unwrap() =
            jsonwebtoken::encode(&header, &claims, &EncodingKey::from_rsa_pem(key.as_bytes()).unwrap()).unwrap();
    }

    fn claims(&self, sub: &str, email: &str, nonce: &str) -> Value {
        let now = peter_server::security::now();
        json!({ "iss": self.issuer(), "aud": CLIENT_ID, "sub": sub, "email": email, "name": "Sam Okta",
                "nonce": nonce, "iat": now, "exp": now + 300, "auth_time": now })
    }
}

async fn api() -> Api {
    let mail = Arc::new(common::CaptureMailer::default());
    let state = AppState::connect(Config::for_tests(), mail.clone()).await.unwrap();
    Api { app: peter_server::app(state), mail }
}

fn query(url: &str, name: &str) -> String {
    url::Url::parse(url).unwrap().query_pairs().find(|(k, _)| k == name).map(|(_, v)| v.to_string()).unwrap_or_default()
}

async fn configure(api: &Api, admin: &str, idp: &Idp, extra: Value) -> Value {
    let mut body = json!({ "issuer": idp.issuer(), "client_id": CLIENT_ID, "client_secret": "s3cret-value", "enabled": true });
    for (k, v) in extra.as_object().unwrap() {
        body[k] = v.clone();
    }
    let (s, b) = api.call("PUT", "/v1/workspace/sso", Some(admin), Some(body)).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    b
}

/// Starts a sign-in and comes back from the provider as `sub` / `email`.
async fn sign_in(api: &Api, idp: &Idp, start: Value, sub: &str, email: &str) -> (StatusCode, Value) {
    let (s, started) = api.call("POST", "/v1/auth/sso/start", None, Some(start)).await;
    assert_eq!(s, StatusCode::OK, "{started}");
    let url = started["url"].as_str().unwrap();
    idp.will_answer(idp.claims(sub, email, &query(url, "nonce")), KEY);
    api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "the-code", "state": query(url, "state") }))).await
}

#[tokio::test]
async fn admins_set_up_single_sign_on() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let member = token(&api.add_member(&owner, "ben@example.com", "member").await);

    let (s, _) = api.call("GET", "/v1/workspace/sso", Some(&member), None).await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, before) = api.call("GET", "/v1/workspace/sso", Some(&owner), None).await;
    assert_eq!(before["configured"], false);
    assert!(before["redirect_uri"].as_str().unwrap().ends_with("/sso/callback"));

    // A secret is needed the first time; an unreachable issuer cannot be switched on; requiring needs it on.
    let base = json!({ "issuer": idp.issuer(), "client_id": CLIENT_ID, "enabled": true });
    let (s, b) = api.call("PUT", "/v1/workspace/sso", Some(&owner), Some(base.clone())).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("invalid_sso")));
    let (s, b) = api.call("PUT", "/v1/workspace/sso", Some(&owner),
        Some(json!({ "issuer": "http://127.0.0.1:9", "client_id": "x", "client_secret": "y", "enabled": true }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("sso_check_failed")));
    let (s, _) = api.call("PUT", "/v1/workspace/sso", Some(&owner),
        Some(json!({ "issuer": idp.issuer(), "client_id": "x", "client_secret": "y", "enabled": false, "enforce": true }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);

    let saved = configure(&api, &owner, &idp, json!({ "domains": "@Example.com, acme.org", "default_role": "viewer" })).await;
    assert_eq!(saved["enabled"], true);
    assert_eq!(saved["domains"], json!(["example.com", "acme.org"]));
    assert!(!saved.to_string().contains("s3cret-value"), "secret exposed");
    // The secret is kept when left out.
    let (s, _) = api.call("PUT", "/v1/workspace/sso", Some(&owner), Some(base)).await;
    assert_eq!(s, StatusCode::OK);
    let (_, checked) = api.call("POST", "/v1/workspace/sso/check", Some(&owner), None).await;
    assert_eq!(checked["ok"], true, "{checked}");

    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&owner), None).await;
    assert_eq!(audit["events"][0]["action"], "workspace.sso_changed");
    assert!(!audit.to_string().contains("s3cret-value"));

    let (s, _) = api.call("DELETE", "/v1/workspace/sso", Some(&owner), None).await;
    assert_eq!(s, StatusCode::NO_CONTENT);
    let (s, _) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "email": "ben@example.com" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}

#[tokio::test]
async fn a_member_signs_in_and_stays_in_that_workspace() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    api.add_member(&owner, "ben@example.com", "member").await;
    configure(&api, &owner, &idp, json!({})).await;
    // Ben also has a workspace of his own, which Acme's provider must not reach.
    let ben_pw = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "ben@example.com", "password": "another-pass-1" }))).await.1;
    let (_, own) = api.call("POST", "/v1/workspaces", Some(&token(&ben_pw)), Some(json!({ "name": "Ben's own" }))).await;
    let own_id = own["workspace"]["id"].as_str().or(own["id"].as_str()).unwrap().to_string();

    // What Peter sends the person to the provider with.
    let (_, started) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "email": "Ben@Example.com" }))).await;
    let url = started["url"].as_str().unwrap().to_string();
    assert!(url.starts_with(&format!("{}/authorize?", idp.issuer())));
    assert_eq!(query(&url, "client_id"), CLIENT_ID);
    assert_eq!(query(&url, "response_type"), "code");
    assert_eq!(query(&url, "code_challenge_method"), "S256");
    assert_eq!(query(&url, "login_hint"), "ben@example.com");
    assert!(query(&url, "scope").contains("openid"));

    idp.will_answer(idp.claims("sub-ben", "ben@example.com", &query(&url, "nonce")), KEY);
    let body = json!({ "code": "the-code", "state": query(&url, "state") });
    let (s, session) = api.call("POST", "/v1/auth/sso/callback", None, Some(body.clone())).await;
    assert_eq!(s, StatusCode::OK, "{session}");
    assert_eq!((session["sso"].as_bool(), session["user"]["email"].as_str()), (Some(true), Some("ben@example.com")));
    assert_eq!(session["workspace"]["name"], "Acme");
    let t = token(&session);

    // The provider received the code, the secret's client, and the verifier matching the challenge.
    let form = idp.received.lock().unwrap().clone();
    let field = |name: &str| form.iter().find(|(k, _)| k == name).map(|(_, v)| v.clone()).unwrap();
    assert_eq!(field("code"), "the-code");
    assert_eq!(field("grant_type"), "authorization_code");
    assert_eq!(peter_server::sso::challenge(&field("code_verifier")), query(&url, "code_challenge"));

    // The same answer cannot be used twice.
    let (s, b) = api.call("POST", "/v1/auth/sso/callback", None, Some(body)).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("sso_expired")));

    let (_, me) = api.call("GET", "/v1/auth/me", Some(&t), None).await;
    assert_eq!(me["sso"], true);
    // Acme's provider vouches for Acme only.
    let (s, b) = api.call("POST", "/v1/auth/switch", Some(&t), Some(json!({ "workspace_id": own_id }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_session")));
    // Refreshing keeps the session a single sign-on one.
    let (_, refreshed) = api.call("POST", "/v1/auth/refresh", None, Some(json!({ "refresh_token": session["refresh_token"] }))).await;
    assert_eq!(refreshed["sso"], true);
    let (s, b) = api.call("POST", "/v1/auth/switch", Some(&token(&refreshed)), Some(json!({ "workspace_id": own_id }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_session")));

    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&owner), None).await;
    assert_eq!((audit["events"][0]["action"].as_str(), audit["events"][0]["detail"]["sso"].as_bool()), (Some("auth.login"), Some(true)));

    // A later sign-in is recognised by the provider's id even if the email changed there.
    let (s, again) = sign_in(&api, &idp, json!({ "email": "ben@example.com" }), "sub-ben", "benjamin@example.com").await;
    assert_eq!((s, again["user"]["email"].as_str()), (StatusCode::OK, Some("ben@example.com")));
}

#[tokio::test]
async fn new_people_join_on_first_sign_in_within_the_rules() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let (_, ws) = api.call("GET", "/v1/workspace", Some(&owner), None).await;
    let wid = ws["id"].as_str().or(ws["workspace"]["id"].as_str()).unwrap().to_string();
    configure(&api, &owner, &idp, json!({ "domains": "example.com", "default_role": "viewer" })).await;

    // An unknown email cannot be looked up; the workspace's link works.
    let (s, _) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "email": "sam@example.com" }))).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
    let (s, session) = sign_in(&api, &idp, json!({ "workspace_id": wid }), "sub-sam", "Sam@Example.com").await;
    assert_eq!(s, StatusCode::OK, "{session}");
    assert_eq!((session["user"]["name"].as_str(), session["workspace"]["role"].as_str()), (Some("Sam Okta"), Some("viewer")));
    let (_, audit) = api.call("GET", "/v1/audit?limit=2", Some(&owner), None).await;
    assert_eq!(audit["events"][1]["action"], "member.joined_by_sso");
    assert_eq!(audit["events"][1]["detail"]["account_created"], true);

    // The new account has no password to guess.
    let (s, _) = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "sam@example.com", "password": "!sso" }))).await;
    assert_eq!(s, StatusCode::UNAUTHORIZED);

    // Other domains are refused, and so are unverified emails.
    let (s, b) = sign_in(&api, &idp, json!({ "workspace_id": wid }), "sub-eve", "eve@elsewhere.com").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_refused")));
    let (_, started) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "workspace_id": wid }))).await;
    let url = started["url"].as_str().unwrap();
    let mut claims = idp.claims("sub-uv", "uv@example.com", &query(url, "nonce"));
    claims["email_verified"] = json!(false);
    idp.will_answer(claims, KEY);
    let (s, _) = api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "c", "state": query(url, "state") }))).await;
    assert_eq!(s, StatusCode::FORBIDDEN);

    // With joining off, only people already in the workspace get in.
    configure(&api, &owner, &idp, json!({ "auto_join": false })).await;
    let (s, b) = sign_in(&api, &idp, json!({ "workspace_id": wid }), "sub-new", "new@example.com").await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_refused")));
    let (s, _) = sign_in(&api, &idp, json!({ "workspace_id": wid }), "sub-sam", "sam@example.com").await;
    assert_eq!(s, StatusCode::OK);
    let (_, members) = api.call("GET", "/v1/members", Some(&owner), None).await;
    assert_eq!(members["members"].as_array().unwrap().len(), 2);
}

#[tokio::test]
async fn answers_that_cannot_be_trusted_are_refused() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    configure(&api, &owner, &idp, json!({})).await;

    let now = peter_server::security::now();
    type Change = fn(&mut Value, i64);
    let cases: [(&str, Change, &str); 5] = [
        ("signed with another key", |_, _| {}, OTHER_KEY),
        ("for another sign-in", |c, _| c["nonce"] = json!("someone-else"), KEY),
        ("for another application", |c, _| c["aud"] = json!("other-client"), KEY),
        ("from another issuer", |c, _| c["iss"] = json!("https://evil.example.com"), KEY),
        ("expired", |c, now| c["exp"] = json!(now - 3600), KEY),
    ];
    for (what, change, key) in cases {
        let (_, started) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "email": "ana@example.com" }))).await;
        let url = started["url"].as_str().unwrap();
        let mut claims = idp.claims("sub-ana", "ana@example.com", &query(url, "nonce"));
        change(&mut claims, now);
        idp.will_answer(claims, key);
        let (s, b) = api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "c", "state": query(url, "state") }))).await;
        assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::BAD_GATEWAY, Some("sso_failed")), "{what}: {b}");
        assert!(b.get("access_token").is_none());
    }
    // An unsigned token is refused too.
    let (_, started) = api.call("POST", "/v1/auth/sso/start", None, Some(json!({ "email": "ana@example.com" }))).await;
    let url = started["url"].as_str().unwrap();
    let b64 = |v: Value| { use base64::Engine; base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(v.to_string()) };
    *idp.id_token.lock().unwrap() =
        format!("{}.{}.", b64(json!({ "alg": "none", "typ": "JWT" })), b64(idp.claims("sub-ana", "ana@example.com", &query(url, "nonce"))));
    let (s, _) = api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "c", "state": query(url, "state") }))).await;
    assert_eq!(s, StatusCode::BAD_GATEWAY);
    // An unknown state never reaches the provider.
    let (s, _) = api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "c", "state": "made-up" }))).await;
    assert_eq!(s, StatusCode::BAD_REQUEST);
}

#[tokio::test]
async fn requiring_single_sign_on_closes_password_access_except_for_owners() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    let admin = token(&api.add_member(&owner, "ada@example.com", "admin").await);
    let ben = token(&api.add_member(&owner, "ben@example.com", "member").await);
    configure(&api, &owner, &idp, json!({})).await;

    // An admin must have used it before requiring it; an owner keeps password access, so may.
    let enforce = json!({ "issuer": idp.issuer(), "client_id": CLIENT_ID, "enabled": true, "enforce": true });
    let (s, b) = api.call("PUT", "/v1/workspace/sso", Some(&admin), Some(enforce.clone())).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::CONFLICT, Some("sso_untested")));
    let (s, ada) = sign_in(&api, &idp, json!({ "email": "ada@example.com" }), "sub-ada", "ada@example.com").await;
    assert_eq!(s, StatusCode::OK);
    let (s, b) = api.call("PUT", "/v1/workspace/sso", Some(&token(&ada)), Some(enforce)).await;
    assert_eq!(s, StatusCode::OK, "{b}");

    // Ben's password session stops working at once, and his password no longer signs him in.
    let (s, b) = api.call("GET", "/v1/projects", Some(&ben), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_required")));
    let (s, b) = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "ben@example.com", "password": "another-pass-1" }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_required")));
    let (s, ben_sso) = sign_in(&api, &idp, json!({ "email": "ben@example.com" }), "sub-ben", "ben@example.com").await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("GET", "/v1/projects", Some(&token(&ben_sso)), None).await;
    assert_eq!(s, StatusCode::OK);

    // The owner can still use a password, so a broken provider cannot lock the workspace.
    let (s, _) = api.call("GET", "/v1/projects", Some(&owner), None).await;
    assert_eq!(s, StatusCode::OK);
    let (s, _) = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "ana@example.com", "password": "correct-horse-1" }))).await;
    assert_eq!(s, StatusCode::OK);

    // Someone with another workspace still reaches that one by password.
    let (s, cat) = api.call("POST", "/v1/auth/signup", None,
        Some(json!({ "email": "cat@example.com", "password": "correct-horse-1", "name": "Cat", "workspace_name": "Cat's" }))).await;
    assert_eq!(s, StatusCode::CREATED);
    let (s, joined) = sign_in(&api, &idp, json!({ "email": "ana@example.com" }), "sub-cat", "cat@example.com").await;
    assert_eq!((s, joined["workspace"]["name"].as_str()), (StatusCode::OK, Some("Acme")));
    let (s, login) = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "cat@example.com", "password": "correct-horse-1" }))).await;
    assert_eq!((s, login["workspace"]["name"].as_str()), (StatusCode::OK, Some("Cat's")), "{login}");
    let (_, acme) = api.call("GET", "/v1/workspace", Some(&owner), None).await;
    let acme_id = acme["id"].as_str().or(acme["workspace"]["id"].as_str()).unwrap();
    let (s, b) = api.call("POST", "/v1/auth/switch", Some(&token(&login)), Some(json!({ "workspace_id": acme_id }))).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_required")));
    let _ = cat;
}

#[tokio::test]
async fn two_factor_requirement_is_left_to_the_provider() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    api.add_member(&owner, "ben@example.com", "member").await;
    configure(&api, &owner, &idp, json!({})).await;
    sqlx_free_require_mfa(&api, &owner).await;
    let (s, ben) = sign_in(&api, &idp, json!({ "email": "ben@example.com" }), "sub-ben", "ben@example.com").await;
    assert_eq!((s, ben["mfa_setup_required"].as_bool()), (StatusCode::OK, Some(false)));
    let (s, _) = api.call("GET", "/v1/projects", Some(&token(&ben)), None).await;
    assert_eq!(s, StatusCode::OK);
    // A provider-vouched session cannot touch his own two-factor settings, or sessions elsewhere.
    let (s, b) = api.call("POST", "/v1/auth/mfa/setup", Some(&token(&ben)), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_session")));
    // His password session is still held to it.
    let (_, pw) = api.call("POST", "/v1/auth/login", None, Some(json!({ "email": "ben@example.com", "password": "another-pass-1" }))).await;
    let (s, b) = api.call("GET", "/v1/projects", Some(&token(&pw)), None).await;
    assert_eq!((s, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("mfa_setup_required")));
}

/// Turns on the workspace's two-factor requirement the way an owner would: set it up, then require it.
async fn sqlx_free_require_mfa(api: &Api, owner: &str) {
    use peter_server::mfa;
    let (s, setup) = api.call("POST", "/v1/auth/mfa/setup", Some(owner), None).await;
    assert_eq!(s, StatusCode::OK, "{setup}");
    let secret = mfa::base32_decode(setup["secret"].as_str().unwrap()).unwrap();
    let code = mfa::code_at(&secret, peter_server::security::now() / mfa::STEP_SECS, mfa::DIGITS);
    let (s, b) = api.call("POST", "/v1/auth/mfa/enable", Some(owner), Some(json!({ "code": code }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
    let (s, b) = api.call("PUT", "/v1/workspace/security", Some(owner), Some(json!({ "require_mfa": true }))).await;
    assert_eq!(s, StatusCode::OK, "{b}");
}

#[tokio::test]
async fn signatures_are_confirmed_at_the_provider() {
    let (api, idp) = (api().await, idp().await);
    let owner = token(&api.signup("ana@example.com", "Acme").await);
    api.add_member(&owner, "ben@example.com", "member").await;
    let (_, ws) = api.call("GET", "/v1/workspace", Some(&owner), None).await;
    let wid = ws["id"].as_str().or(ws["workspace"]["id"].as_str()).unwrap().to_string();
    configure(&api, &owner, &idp, json!({})).await;
    // Sam has no Peter password at all.
    let (_, sam) = sign_in(&api, &idp, json!({ "workspace_id": wid }), "sub-sam", "sam@example.com").await;
    let sam = token(&sam);

    let (_, s) = api.call("POST", "/v1/sessions", Some(&sam), Some(json!({ "title": "S", "validation_type": "initial" }))).await;
    let sid = s["session"]["id"].as_str().unwrap().to_string();
    let (st, _) = api.call("POST", &format!("/v1/sessions/{sid}/documents/observe/draft"), Some(&sam), Some(json!({ "provider": "sample" }))).await;
    assert_eq!(st, StatusCode::ACCEPTED);
    for _ in 0..100 {
        let (_, doc) = api.call("GET", &format!("/v1/sessions/{sid}/documents/observe"), Some(&sam), None).await;
        if doc["document"]["status"] != "drafting" { break; }
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
    }
    let path = format!("/v1/sessions/{sid}/documents/observe/signatures");
    let (st, b) = api.call("POST", &path, Some(&sam), Some(json!({ "version": 1, "meaning": "reviewed", "password": "anything-at-all" }))).await;
    assert_eq!((st, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("wrong_password")));

    // Sam confirms at the provider, which is asked to make him sign in again.
    let confirm = async |as_sub: &str, as_email: &str, auth_time: i64| {
        let (st, started) = api.call("POST", "/v1/auth/sso/sign-start", Some(&sam), None).await;
        assert_eq!(st, StatusCode::OK, "{started}");
        let url = started["url"].as_str().unwrap().to_string();
        assert_eq!((query(&url, "prompt"), query(&url, "max_age")), ("login".to_string(), "0".to_string()));
        let mut claims = idp.claims(as_sub, as_email, &query(&url, "nonce"));
        claims["auth_time"] = json!(auth_time);
        idp.will_answer(claims, KEY);
        api.call("POST", "/v1/auth/sso/callback", None, Some(json!({ "code": "c", "state": query(&url, "state") }))).await
    };
    let now = peter_server::security::now();
    // Someone else at the keyboard, or a stale sign-in, confirms nothing.
    let (st, b) = confirm("sub-ben", "ben@example.com", now).await;
    assert_eq!((st, b["error"]["code"].as_str()), (StatusCode::FORBIDDEN, Some("sso_refused")));
    let (st, _) = confirm("sub-sam", "sam@example.com", now - 3600).await;
    assert_eq!(st, StatusCode::FORBIDDEN);

    let (st, ok) = confirm("sub-sam", "sam@example.com", now).await;
    assert_eq!((st, ok["purpose"].as_str()), (StatusCode::OK, Some("sign")), "{ok}");
    assert!(ok.get("access_token").is_none(), "a confirmation is not a session");
    let ticket = ok["sign_token"].as_str().unwrap();
    // Another person cannot spend Sam's confirmation.
    let (st, _) = api.call("POST", &path, Some(&owner), Some(json!({ "version": 1, "meaning": "reviewed", "sso_token": ticket }))).await;
    assert_eq!(st, StatusCode::BAD_REQUEST);
    let (st, signed) = api.call("POST", &path, Some(&sam), Some(json!({ "version": 1, "meaning": "reviewed", "sso_token": ticket }))).await;
    assert_eq!(st, StatusCode::CREATED, "{signed}");
    // It signs once.
    let (st, b) = api.call("POST", &path, Some(&sam), Some(json!({ "version": 1, "meaning": "approved", "sso_token": ticket }))).await;
    assert_eq!((st, b["error"]["code"].as_str()), (StatusCode::BAD_REQUEST, Some("sso_confirmation_expired")));
    let (_, audit) = api.call("GET", "/v1/audit?limit=1", Some(&owner), None).await;
    assert_eq!(audit["events"][0]["detail"]["confirmed_by"], "sso");
}
