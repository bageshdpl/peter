mod audit;
mod auth;
mod extension;
mod members;
mod observations;
mod projects;
mod sessions;
mod sources;
mod workspace;

use axum::{
    Router,
    extract::{DefaultBodyLimit, FromRequest},
    routing::{delete, get, patch, post},
};
use serde::Serialize;
use sqlx::SqliteConnection;

use crate::{
    auth::Tokens,
    error::{AppError, AppResult},
    state::AppState,
};

pub fn router(max_upload_bytes: usize) -> Router<AppState> {
    Router::new()
        .route("/auth/signup", post(auth::signup))
        .route("/auth/login", post(auth::login))
        .route("/auth/refresh", post(auth::refresh))
        .route("/auth/logout", post(auth::logout))
        .route("/auth/me", get(auth::me))
        .route("/auth/workspaces", get(auth::workspaces))
        .route("/auth/switch", post(auth::switch))
        .route("/auth/change-password", post(auth::change_password))
        .route("/auth/reset/request", post(auth::reset_request))
        .route("/auth/reset/confirm", post(auth::reset_confirm))
        .route("/auth/accept-invite", post(members::accept_invite))
        .route("/workspaces", post(workspace::create))
        .route("/workspace", get(workspace::get).patch(workspace::rename))
        .route("/members", get(members::list).post(members::invite))
        .route("/members/{user_id}", patch(members::change_role).delete(members::remove))
        .route("/invites/{invite_id}", delete(members::revoke_invite))
        .route("/audit", get(audit::list))
        .route("/validation-types", get(sessions::types))
        .route("/projects", get(projects::list).post(projects::create))
        .route("/projects/{id}", get(projects::get).patch(projects::update))
        .route("/projects/{id}/archive", post(projects::archive))
        .route("/projects/{id}/restore", post(projects::restore))
        .route(
            "/projects/{id}/sources",
            get(sources::list)
                .post(sources::upload)
                // Room for the multipart framing around the largest allowed file.
                .layer(DefaultBodyLimit::max(max_upload_bytes + 64 * 1024)),
        )
        .route("/projects/{id}/sources/{source_id}", delete(sources::remove))
        .route("/projects/{id}/sources/{source_id}/file", get(sources::download))
        .route("/sessions", get(sessions::list).post(sessions::create))
        .route("/sessions/{id}", get(sessions::get).patch(sessions::update))
        .route("/sessions/{id}/archive", post(sessions::archive))
        .route("/sessions/{id}/restore", post(sessions::restore))
        .route("/auth/extension-code", post(extension::create_code))
        .route("/auth/extension-exchange", post(extension::exchange))
        .route(
            "/sessions/{id}/observations",
            get(observations::list)
                .post(observations::create)
                // A screenshot is sent base64-encoded inside the JSON.
                .layer(DefaultBodyLimit::max(observations::MAX_SCREENSHOT_BYTES / 3 * 4 + 64 * 1024)),
        )
        .route("/sessions/{id}/observations/{observation_id}", delete(observations::remove))
        .route("/sessions/{id}/observations/{observation_id}/screenshot", get(observations::screenshot))
        .route("/sessions/{id}/events", get(observations::events))
}

/// JSON body extractor whose errors use the API's error shape.
#[derive(FromRequest)]
#[from_request(via(axum::Json), rejection(AppError))]
pub struct Body<T>(pub T);

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct UserView {
    pub id: String,
    pub email: String,
    pub name: String,
}

#[derive(Debug, Serialize, sqlx::FromRow)]
pub struct WorkspaceView {
    pub id: String,
    pub name: String,
    pub role: String,
}

/// What every successful sign-in, switch or refresh returns.
#[derive(Debug, Serialize)]
pub struct SessionView {
    #[serde(flatten)]
    pub tokens: Tokens,
    pub user: UserView,
    pub workspace: WorkspaceView,
    pub workspaces: Vec<WorkspaceView>,
}

pub async fn user_workspaces(conn: &mut SqliteConnection, user_id: &str) -> AppResult<Vec<WorkspaceView>> {
    Ok(sqlx::query_as(
        "SELECT w.id, w.name, m.role FROM memberships m JOIN workspaces w ON w.id = m.workspace_id
         WHERE m.user_id = ? ORDER BY w.name COLLATE NOCASE, w.id",
    )
    .bind(user_id)
    .fetch_all(&mut *conn)
    .await?)
}

pub async fn session_view(
    conn: &mut SqliteConnection,
    tokens: Tokens,
    user_id: &str,
    workspace_id: &str,
) -> AppResult<SessionView> {
    let user: UserView = sqlx::query_as("SELECT id, email, name FROM users WHERE id = ?")
        .bind(user_id)
        .fetch_one(&mut *conn)
        .await?;
    let workspaces = user_workspaces(conn, user_id).await?;
    let workspace = workspaces
        .iter()
        .find(|w| w.id == workspace_id)
        .map(|w| WorkspaceView { id: w.id.clone(), name: w.name.clone(), role: w.role.clone() })
        .ok_or_else(|| AppError::forbidden("You are not a member of this workspace."))?;
    Ok(SessionView { tokens, user, workspace, workspaces })
}

pub fn clean_name(value: &str, field: &'static str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 120 {
        return Err(AppError::bad_request("invalid_name", format!("Enter a {field} of 1 to 120 characters.")));
    }
    Ok(value.to_string())
}
