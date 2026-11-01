//! Hosting: files kept in the database, invite-only sign-up with an admin
//! bootstrap, public server facts, and the extension download.

mod common;

use std::{io::Read, sync::Arc};

use axum::http::StatusCode;
use common::{Api, token};
use peter_server::state::{AppState, Config};
use serde_json::{Value, json};

async fn api_with(adjust: impl FnOnce(&mut Config)) -> (Api, Config) {
    let mut config = Config::for_tests();
    adjust(&mut config);
    let mail = Arc::new(common::CaptureMailer::default());
    let state = AppState::connect(config.clone(), mail.clone()).await.unwrap();
    (Api { app: peter_server::app(state), mail }, config)
}

async fn signup(api: &Api, email: &str) -> (StatusCode, Value) {
    api.call("POST", "/v1/auth/signup", None,
        Some(json!({ "email": email, "password": "long-enough-1", "name": "A", "workspace_name": "W" }))).await
}

#[tokio::test]
async fn invite_only_lets_only_the_admin_create_the_first_account() {
    let (api, _) = api_with(|c| {
        c.allow_signup = false;
        c.admin_email = Some("owner@example.com".into());
    })
    .await;
    let (_, meta) = api.call("GET", "/v1/meta", None, None).await;
    assert_eq!(meta["signup"], "first_admin");

    let (s, b) = signup(&api, "stranger@example.com").await;
    assert_eq!((s, b["error"]["message"].as_str().unwrap().contains("invite-only")), (StatusCode::FORBIDDEN, true));
    let (s, owner) = signup(&api, "Owner@Example.com").await;
    assert_eq!(s, StatusCode::CREATED, "{owner}");
    // After that, nobody else can sign up: invites only.
    let (s, _) = signup(&api, "owner2@example.com").await;
    assert_eq!(s, StatusCode::FORBIDDEN);
    let (_, meta) = api.call("GET", "/v1/meta", None, None).await;
    assert_eq!(meta["signup"], "invite_only");
    // Invites still work.
    let joined = api.add_member(&token(&owner), "tester@example.com", "member").await;
    assert_eq!(joined["user"]["email"], "tester@example.com");
}

#[tokio::test]
async fn without_an_admin_email_invite_only_allows_no_sign_up() {
    let (api, _) = api_with(|c| c.allow_signup = false).await;
    assert_eq!(signup(&api, "a@example.com").await.0, StatusCode::FORBIDDEN);
    let (_, meta) = api.call("GET", "/v1/meta", None, None).await;
    assert_eq!(meta["signup"], "invite_only");
}

#[tokio::test]
async fn files_live_in_the_database() {
    let (api, config) = api_with(|_| {}).await;
    let t = token(&api.signup("ana@example.com", "Acme").await);
    let (_, p) = api.call("POST", "/v1/projects", Some(&t), Some(json!({ "name": "LIMS" }))).await;
    let pid = p["id"].as_str().unwrap();
    let (_, up) = api.upload(&format!("/v1/projects/{pid}/sources"), &t, "sop.txt", b"Stored in the database.").await;
    let sid = up["source"]["id"].as_str().unwrap();
    // Nothing is written as a loose file any more.
    assert!(!config.data_dir.join("sources").join(sid).exists());
    let (s, _, bytes) = api.raw_get(&format!("/v1/projects/{pid}/sources/{sid}/file"), &t).await;
    assert_eq!((s, bytes.as_slice()), (StatusCode::OK, b"Stored in the database.".as_slice()));

    // Screenshots too.
    let (_, s2) = api.call("POST", "/v1/sessions", Some(&t), Some(json!({ "title": "S", "validation_type": "initial" }))).await;
    let session = s2["session"]["id"].as_str().unwrap();
    let jpeg = include_bytes!("fixtures/screen.jpg");
    let data_url = format!("data:image/jpeg;base64,{}", base64::Engine::encode(&base64::engine::general_purpose::STANDARD, jpeg));
    let (_, o) = api.call("POST", &format!("/v1/sessions/{session}/observations"), Some(&t),
        Some(json!({ "kind": "capture", "screenshot": data_url }))).await;
    let oid = o["id"].as_str().unwrap();
    assert!(!config.data_dir.join("screenshots").join(oid).exists());
    let (s, _, shot) = api.raw_get(&format!("/v1/sessions/{session}/observations/{oid}/screenshot"), &t).await;
    assert_eq!((s, shot.len()), (StatusCode::OK, jpeg.len()));
}

#[tokio::test]
async fn the_extension_download_points_at_this_server() {
    let ext = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../extension");
    let (api, _) = api_with(|c| {
        c.extension_dir = Some(ext.clone());
        c.app_url = "https://peter-test.onrender.com/".into();
    })
    .await;
    let (_, meta) = api.call("GET", "/v1/meta", None, None).await;
    assert_eq!(meta["extension_download"], true);

    let req = axum::http::Request::builder().uri("/v1/extension/download").body(axum::body::Body::empty()).unwrap();
    let res = tower::ServiceExt::oneshot(api.app.clone(), req).await.unwrap();
    assert_eq!(res.status(), StatusCode::OK);
    let bytes = http_body_util::BodyExt::collect(res.into_body()).await.unwrap().to_bytes();
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(bytes.to_vec())).unwrap();
    let read = |zip: &mut zip::ZipArchive<std::io::Cursor<Vec<u8>>>, name: &str| {
        let mut s = String::new();
        zip.by_name(name).unwrap().read_to_string(&mut s).unwrap();
        s
    };
    assert!(read(&mut zip, "peter-observer/config.js").contains(r#"export const PETER_URL = "https://peter-test.onrender.com";"#));
    let manifest: Value = serde_json::from_str(&read(&mut zip, "peter-observer/manifest.json")).unwrap();
    assert_eq!(manifest["host_permissions"][0], "https://peter-test.onrender.com/*");
    assert_eq!(manifest["content_scripts"][0]["matches"][0], "https://peter-test.onrender.com/*");
    assert!(zip.by_name("peter-observer/background.js").is_ok());
    assert!(zip.by_name("peter-observer/icons/icon-128.png").is_ok());
    assert!(zip.by_name("peter-observer/README.md").is_err());

    // A server without the extension folder says so.
    let (plain, _) = api_with(|_| {}).await;
    let (s, _) = plain.call("GET", "/v1/extension/download", None, None).await;
    assert_eq!(s, StatusCode::NOT_FOUND);
}
