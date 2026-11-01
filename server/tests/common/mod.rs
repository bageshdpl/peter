#![allow(dead_code)]
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
pub struct CaptureMailer(pub Mutex<Vec<(String, String)>>);

impl Mailer for CaptureMailer {
    fn send_link(&self, to: &str, _subject: &str, link: &str) {
        self.0.lock().unwrap().push((to.to_string(), link.to_string()));
    }
}

impl CaptureMailer {
    pub fn last_token_for(&self, to: &str) -> String {
        let sent = self.0.lock().unwrap();
        let (_, link) = sent.iter().rev().find(|(t, _)| t == to).expect("no email sent");
        link.split("#token=").nth(1).unwrap().to_string()
    }
}

pub struct Api {
    pub app: Router,
    pub mail: Arc<CaptureMailer>,
}

impl Api {
    pub async fn new() -> Self {
        let mail = Arc::new(CaptureMailer::default());
        let state = AppState::connect(Config::for_tests(), mail.clone()).await.unwrap();
        Self { app: peter_server::app(state), mail }
    }

    pub async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (StatusCode, Value) {
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

    pub async fn signup(&self, email: &str, workspace: &str) -> Value {
        let (status, body) = self
            .call("POST", "/v1/auth/signup", None, Some(json!({
                "email": email, "password": "correct-horse-1", "name": "Test User", "workspace_name": workspace
            })))
            .await;
        assert_eq!(status, StatusCode::CREATED, "{body}");
        body
    }

    /// An owner invites `email` with `role`; the invitee creates an account.
    pub async fn add_member(&self, owner_token: &str, email: &str, role: &str) -> Value {
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

pub fn token(session: &Value) -> String {
    session["access_token"].as_str().unwrap().to_string()
}


impl Api {
    /// Uploads `bytes` as a multipart form field named `file`.
    pub async fn upload(&self, path: &str, token: &str, filename: &str, bytes: &[u8]) -> (StatusCode, Value) {
        let boundary = "peter-test-boundary";
        let mut body = format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"{filename}\"\r\nContent-Type: application/octet-stream\r\n\r\n"
        )
        .into_bytes();
        body.extend_from_slice(bytes);
        body.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
        let req = Request::builder()
            .method("POST")
            .uri(path)
            .header(header::AUTHORIZATION, format!("Bearer {token}"))
            .header(header::CONTENT_TYPE, format!("multipart/form-data; boundary={boundary}"))
            .body(Body::from(body))
            .unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let status = res.status();
        let bytes = res.into_body().collect().await.unwrap().to_bytes();
        (status, serde_json::from_slice(&bytes).unwrap_or(Value::Null))
    }

    /// GET returning the raw response (for downloads).
    pub async fn raw_get(&self, path: &str, token: &str) -> (StatusCode, axum::http::HeaderMap, Vec<u8>) {
        let req = Request::builder().uri(path).header(header::AUTHORIZATION, format!("Bearer {token}")).body(Body::empty()).unwrap();
        let res = self.app.clone().oneshot(req).await.unwrap();
        let (status, headers) = (res.status(), res.headers().clone());
        (status, headers, res.into_body().collect().await.unwrap().to_bytes().to_vec())
    }
}
