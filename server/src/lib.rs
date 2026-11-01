//! Peter API server.

pub mod ai;
pub mod auth;
pub mod catalog;
pub mod controls;
pub mod drafting;
pub mod error;
pub mod export;
pub mod extract;
pub mod mfa;
pub mod prompts;
pub mod routes;
pub mod security;
pub mod standards;
pub mod storage;
pub mod state;
pub mod throttle;
pub mod trace;

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    routing::get,
};
use tower_http::{
    cors::CorsLayer,
    services::{ServeDir, ServeFile},
    trace::TraceLayer,
};

use crate::state::AppState;

pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(state.config.app_url.parse::<HeaderValue>().ok().into_iter().collect::<Vec<_>>())
        .allow_methods([Method::GET, Method::POST, Method::PUT, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    let mut router = Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/v1", routes::router(state.config.max_upload_bytes));

    // In the ready-to-run package the server also serves the web app, so
    // nothing else needs installing. Unknown paths get index.html, which
    // lets the app's own pages (like /sessions/...) load directly.
    if let Some(dir) = state.config.web_dir.as_ref().filter(|d| d.join("index.html").is_file()) {
        let index = ServeFile::new(dir.join("index.html"));
        router = router.fallback_service(ServeDir::new(dir).fallback(index));
    }

    router.layer(cors).layer(TraceLayer::new_for_http()).with_state(state)
}
