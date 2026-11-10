mod ai_settings;
mod audit;
mod auth;
mod controls;
mod documents;
mod extension;
mod library;
mod members;
mod meta;
mod observations;
mod projects;
mod review;
mod security;
mod sessions;
mod sharing;
mod style;
mod sources;
mod sso;
mod trace;
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
        .route("/sessions/{id}/current", post(documents::set_current))
        .route("/sessions/{id}/steps/{step_id}", axum::routing::put(documents::set_step_status))
        .route("/ai/providers", get(documents::providers))
        .route("/sessions/{id}/documents/{step_id}", get(documents::get).put(documents::save))
        .route("/sessions/{id}/documents/{step_id}/draft", post(documents::start_draft))
        .route("/sessions/{id}/documents/{step_id}/versions/{version}", get(documents::get_version))
        .route("/sessions/{id}/documents/{step_id}/comments", get(review::list_comments).post(review::add_comment))
        .route("/comments/{comment_id}/resolve", post(review::resolve_comment))
        .route("/comments/{comment_id}/reopen", post(review::reopen_comment))
        .route("/sessions/{id}/documents/{step_id}/signatures", post(review::sign))
        .route("/sessions/{id}/documents/{step_id}/export/docx", get(review::export_docx))
        .route("/sessions/{id}/documents/{step_id}/export/xlsx", get(review::export_xlsx))
        .route("/sessions/{id}/documents/{step_id}/export/pdf", get(review::export_pdf))
        .route("/sessions/{id}/export", get(review::export_package))
        .route("/sessions/{id}/trace", get(trace::get))
        .route("/sessions/{id}/trace/write", post(trace::write))
        .route("/sessions/{id}/trace/export", get(trace::export_xlsx))
        .route("/auth/login/mfa", post(auth::login_mfa))
        .route("/auth/mfa", get(security::status))
        .route("/auth/mfa/setup", post(security::setup))
        .route("/auth/mfa/enable", post(security::enable))
        .route("/auth/mfa/disable", post(security::disable))
        .route("/auth/mfa/recovery-codes", post(security::regenerate_codes))
        .route("/auth/sessions", get(security::list_sessions))
        .route("/auth/sessions/revoke-others", post(security::revoke_other_sessions))
        .route("/auth/sessions/{session_id}", delete(security::revoke_session))
        .route("/workspace/security", axum::routing::put(security::set_policy))
        .route("/library/standards", get(library::list_standards).put(library::set_standards))
        .route(
            "/library/templates",
            get(library::list_templates).post(library::upload_template).layer(DefaultBodyLimit::max(10 * 1024 * 1024 + 64 * 1024)),
        )
        .route("/library/templates/{template_id}", get(library::get_template).delete(library::remove_template))
        .route("/workspace/branding", get(library::get_branding).put(library::set_branding))
        .route("/projects/{id}/controls", get(controls::list).post(controls::create))
        .route("/projects/{id}/controls/{control_id}", delete(controls::remove))
        .route("/projects/{id}/sources/{source_id}/controls", post(controls::extract))
        .route("/sessions/{id}/controls", get(controls::coverage))
        .route("/sessions/{id}/controls/export", get(controls::export_xlsx))
        .route("/sessions/{id}/controls/{key}", axum::routing::put(controls::decide))
        .route("/sessions/{id}/shares", get(sharing::list_for_session).post(sharing::create))
        .route("/shares/{share_id}", delete(sharing::revoke))
        .route("/shares/{share_id}/done", post(sharing::set_done))
        .route("/shared-with-me", get(sharing::shared_with_me))
        .route("/notifications", get(sharing::notifications))
        .route("/notifications/unread", get(sharing::unread))
        .route("/notifications/read", post(sharing::mark_read))
        .route("/workspace/ai", get(ai_settings::get))
        .route("/workspace/ai/usage", get(ai_settings::usage))
        .route("/workspace/ai/{provider}", axum::routing::put(ai_settings::set_key).delete(ai_settings::remove_key))
        .route("/workspace/ai/{provider}/check", post(ai_settings::check))
        .route("/library/style", get(style::get).put(style::set_guide))
        .route("/library/style/learn", post(style::learn))
        .route(
            "/library/style/samples",
            post(style::add_sample).layer(DefaultBodyLimit::max(10 * 1024 * 1024 + 64 * 1024)),
        )
        .route("/library/style/samples/{sample_id}", delete(style::remove_sample))
        .route("/workspace/sso", get(sso::get).put(sso::set).delete(sso::remove))
        .route("/workspace/sso/check", post(sso::check))
        .route("/auth/sso/start", post(sso::start))
        .route("/auth/sso/sign-start", post(sso::sign_start))
        .route("/auth/sso/callback", post(sso::callback))
        .route("/meta", get(meta::meta))
        .route("/extension/download", get(meta::extension_zip))
        .fallback(|| async { AppError::not_found("No such API endpoint.") })
        .route(
            "/workspace/branding/logo",
            get(library::get_logo)
                .post(library::upload_logo)
                .delete(library::remove_logo)
                .layer(DefaultBodyLimit::max(library::MAX_LOGO_BYTES + 64 * 1024)),
        )
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
    /// The workspace requires two-factor sign-in and this person has not set it up.
    pub mfa_setup_required: bool,
    /// Signed in through the workspace's identity provider.
    pub sso: bool,
}

pub async fn mfa_setup_required(conn: &mut SqliteConnection, user_id: &str, workspace_id: &str) -> AppResult<bool> {
    let (required, has): (bool, bool) = sqlx::query_as(
        "SELECT w.require_mfa, EXISTS (SELECT 1 FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL)
         FROM workspaces w WHERE w.id = ?",
    )
    .bind(user_id)
    .bind(workspace_id)
    .fetch_one(&mut *conn)
    .await?;
    Ok(required && !has)
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
    let mfa_setup_required = mfa_setup_required(conn, user_id, workspace_id).await?;
    // Callers that know the session came through single sign-on set `sso` (and clear the two-factor prompt).
    Ok(SessionView { tokens, user, workspace, workspaces, mfa_setup_required, sso: false })
}

pub fn clean_name(value: &str, field: &'static str) -> AppResult<String> {
    let value = value.trim();
    if value.is_empty() || value.chars().count() > 120 {
        return Err(AppError::bad_request("invalid_name", format!("Enter a {field} of 1 to 120 characters.")));
    }
    Ok(value.to_string())
}
