//! Peter API server.

pub mod ai;
pub mod auth;
pub mod catalog;
pub mod drafting;
pub mod error;
pub mod export;
pub mod extract;
pub mod prompts;
pub mod routes;
pub mod security;
pub mod state;

use axum::{
    Router,
    http::{HeaderValue, Method, header},
    routing::get,
};
use tower_http::{cors::CorsLayer, trace::TraceLayer};

use crate::state::AppState;

pub fn app(state: AppState) -> Router {
    let cors = CorsLayer::new()
        .allow_origin(state.config.app_url.parse::<HeaderValue>().ok().into_iter().collect::<Vec<_>>())
        .allow_methods([Method::GET, Method::POST, Method::PATCH, Method::DELETE])
        .allow_headers([header::AUTHORIZATION, header::CONTENT_TYPE]);

    Router::new()
        .route("/healthz", get(|| async { "ok" }))
        .nest("/v1", routes::router(state.config.max_upload_bytes))
        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}
