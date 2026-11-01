use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Body, SessionView, clean_name, session_view, user_workspaces};
use crate::{
    auth::{Auth, audit, revoke_user_sessions, rotate_session, start_session},
    error::{AppError, AppResult},
    security::{Role, check_new_password, hash_token, new_id, new_token, normalize_email, now},
    state::AppState,
};

/// Argon2 is deliberately slow, so it runs off the async worker threads.
pub async fn hash_password(password: String) -> AppResult<String> {
    tokio::task::spawn_blocking(move || crate::security::hash_password(&password))
        .await
        .map_err(|_| AppError::internal())?
}

pub async fn verify_password(password: String, hash: String) -> AppResult<bool> {
    tokio::task::spawn_blocking(move || crate::security::verify_password(&password, &hash))
        .await
        .map_err(|_| AppError::internal())
}

#[derive(Deserialize)]
pub struct SignupBody {
    email: String,
    password: String,
    name: String,
    workspace_name: String,
}

pub async fn signup(
    State(state): State<AppState>,
    Body(body): Body<SignupBody>,
) -> AppResult<(StatusCode, Json<SessionView>)> {
    if !state.config.allow_signup {
        return Err(AppError::forbidden("Sign-up is turned off. Ask an administrator for an invite."));
    }
    let email = normalize_email(&body.email)?;
    let name = clean_name(&body.name, "name")?;
    let workspace_name = clean_name(&body.workspace_name, "workspace name")?;
    check_new_password(&body.password)?;
    let password_hash = hash_password(body.password).await?;

    let mut tx = state.db.begin().await?;
    let taken: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&mut *tx).await?;
    if taken.is_some() {
        return Err(AppError::conflict("email_taken", "An account with this email already exists. Sign in instead."));
    }

    let (user_id, workspace_id, t) = (new_id(), new_id(), now());
    sqlx::query("INSERT INTO users (id, email, name, password_hash, created_at) VALUES (?, ?, ?, ?, ?)")
        .bind(&user_id)
        .bind(&email)
        .bind(&name)
        .bind(&password_hash)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO workspaces (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&workspace_id)
        .bind(&workspace_name)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)")
        .bind(&workspace_id)
        .bind(&user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "workspace.created", Some(&workspace_id),
        json!({ "name": workspace_name, "by_signup": true })).await?;

    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Deserialize)]
pub struct LoginBody {
    email: String,
    password: String,
    workspace_id: Option<String>,
}

pub async fn login(State(state): State<AppState>, Body(body): Body<LoginBody>) -> AppResult<Json<SessionView>> {
    let invalid = || AppError::unauthorized("The email or password is not right.");
    let email = normalize_email(&body.email).map_err(|_| invalid())?;
    let user: Option<(String, String)> = sqlx::query_as("SELECT id, password_hash FROM users WHERE email = ?")
        .bind(&email)
        .fetch_optional(&state.db)
        .await?;
    let (user_id, stored) = match user {
        Some(found) => found,
        None => {
            // Same work as a real check, so timing does not reveal unknown emails.
            verify_password(body.password, state.dummy_hash.to_string()).await?;
            return Err(invalid());
        }
    };
    if !verify_password(body.password, stored).await? {
        return Err(invalid());
    }

    let mut tx = state.db.begin().await?;
    let workspace_id = match body.workspace_id {
        Some(id) => id,
        None => {
            // The workspace used most recently, or else the first one joined.
            let last: Option<(String,)> = sqlx::query_as(
                "SELECT m.workspace_id FROM memberships m
                 LEFT JOIN sessions s ON s.workspace_id = m.workspace_id AND s.user_id = m.user_id
                 WHERE m.user_id = ?
                 GROUP BY m.workspace_id
                 ORDER BY MAX(COALESCE(s.created_at, 0)) DESC, MIN(m.created_at) ASC
                 LIMIT 1",
            )
            .bind(&user_id)
            .fetch_optional(&mut *tx)
            .await?;
            last.map(|(id,)| id).ok_or_else(|| {
                AppError::new(StatusCode::FORBIDDEN, "no_workspace", "You are not a member of any workspace. Ask for an invite.")
            })?
        }
    };
    ensure_member(&mut tx, &user_id, &workspace_id).await?;

    let tokens = start_session(&mut tx, &state, &user_id, &workspace_id).await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "auth.login", None, json!({})).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

async fn ensure_member(conn: &mut sqlx::SqliteConnection, user_id: &str, workspace_id: &str) -> AppResult<Role> {
    let role: Option<(String,)> = sqlx::query_as("SELECT role FROM memberships WHERE user_id = ? AND workspace_id = ?")
        .bind(user_id)
        .bind(workspace_id)
        .fetch_optional(&mut *conn)
        .await?;
    match role {
        Some((role,)) => Role::parse(&role),
        None => Err(AppError::forbidden("You are not a member of this workspace.")),
    }
}

#[derive(Deserialize)]
pub struct RefreshBody {
    refresh_token: String,
}

pub async fn refresh(State(state): State<AppState>, Body(body): Body<RefreshBody>) -> AppResult<Json<SessionView>> {
    let mut tx = state.db.begin().await?;
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT s.id, s.user_id, s.workspace_id FROM sessions s
         JOIN memberships m ON m.workspace_id = s.workspace_id AND m.user_id = s.user_id
         WHERE s.refresh_hash = ? AND s.revoked_at IS NULL AND s.refresh_expires_at > ?",
    )
    .bind(hash_token(&body.refresh_token))
    .bind(now())
    .fetch_optional(&mut *tx)
    .await?;
    let (session_id, user_id, workspace_id) =
        row.ok_or_else(|| AppError::unauthorized("Your sign-in has expired. Sign in again."))?;
    let tokens = rotate_session(&mut tx, &state, &session_id).await?;
    let view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

pub async fn logout(State(state): State<AppState>, auth: Auth) -> AppResult<StatusCode> {
    sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ?")
        .bind(now())
        .bind(&auth.session_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn me(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let user: super::UserView = sqlx::query_as("SELECT id, email, name FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_one(&mut *conn)
        .await?;
    let workspaces = user_workspaces(&mut conn, &auth.user_id).await?;
    let workspace = workspaces.iter().find(|w| w.id == auth.workspace_id);
    Ok(Json(json!({ "user": user, "workspace": workspace, "workspaces": workspaces })))
}

pub async fn workspaces(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    let workspaces = user_workspaces(&mut conn, &auth.user_id).await?;
    Ok(Json(json!({ "workspaces": workspaces })))
}

#[derive(Deserialize)]
pub struct SwitchBody {
    workspace_id: String,
}

pub async fn switch(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<SwitchBody>,
) -> AppResult<Json<SessionView>> {
    let mut tx = state.db.begin().await?;
    ensure_member(&mut tx, &auth.user_id, &body.workspace_id).await?;
    sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ?")
        .bind(now())
        .bind(&auth.session_id)
        .execute(&mut *tx)
        .await?;
    let tokens = start_session(&mut tx, &state, &auth.user_id, &body.workspace_id).await?;
    let view = session_view(&mut tx, tokens, &auth.user_id, &body.workspace_id).await?;
    tx.commit().await?;
    Ok(Json(view))
}

#[derive(Deserialize)]
pub struct ChangePasswordBody {
    current_password: String,
    new_password: String,
}

pub async fn change_password(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<ChangePasswordBody>,
) -> AppResult<StatusCode> {
    check_new_password(&body.new_password)?;
    let (stored,): (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_one(&state.db)
        .await?;
    if !verify_password(body.current_password, stored).await? {
        return Err(AppError::bad_request("wrong_password", "Your current password is not right."));
    }
    let new_hash = hash_password(body.new_password).await?;

    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&auth.user_id)
        .execute(&mut *tx)
        .await?;
    // Other devices must sign in again; this one stays signed in.
    revoke_user_sessions(&mut tx, &auth.user_id, None, Some(&auth.session_id)).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "user.password_changed",
        Some(&auth.user_id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResetRequestBody {
    email: String,
}

/// Always answers 204, so it does not reveal which emails have accounts.
pub async fn reset_request(
    State(state): State<AppState>,
    Body(body): Body<ResetRequestBody>,
) -> AppResult<StatusCode> {
    let Ok(email) = normalize_email(&body.email) else {
        return Ok(StatusCode::NO_CONTENT);
    };
    let user: Option<(String,)> =
        sqlx::query_as("SELECT id FROM users WHERE email = ?").bind(&email).fetch_optional(&state.db).await?;
    if let Some((user_id,)) = user {
        let token = new_token();
        let t = now();
        sqlx::query(
            "INSERT INTO password_resets (id, user_id, token_hash, created_at, expires_at) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&user_id)
        .bind(hash_token(&token))
        .bind(t)
        .bind(t + state.config.reset_ttl_secs)
        .execute(&state.db)
        .await?;
        // The token sits in the fragment so it never reaches server logs.
        let link = format!("{}/reset#token={token}", state.config.app_url);
        state.mailer.send_link(&email, "Reset your Peter password", &link);
    }
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct ResetConfirmBody {
    token: String,
    password: String,
}

pub async fn reset_confirm(
    State(state): State<AppState>,
    Body(body): Body<ResetConfirmBody>,
) -> AppResult<StatusCode> {
    check_new_password(&body.password)?;
    let new_hash = hash_password(body.password).await?;

    let mut tx = state.db.begin().await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT id, user_id FROM password_resets WHERE token_hash = ? AND used_at IS NULL AND expires_at > ?",
    )
    .bind(hash_token(&body.token))
    .bind(now())
    .fetch_optional(&mut *tx)
    .await?;
    let (reset_id, user_id) =
        row.ok_or_else(|| AppError::bad_request("invalid_token", "This reset link has expired or was already used."))?;

    // Using one link spends every outstanding link for this user.
    sqlx::query("UPDATE password_resets SET used_at = ? WHERE user_id = ? AND used_at IS NULL")
        .bind(now())
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE users SET password_hash = ? WHERE id = ?")
        .bind(new_hash)
        .bind(&user_id)
        .execute(&mut *tx)
        .await?;
    revoke_user_sessions(&mut tx, &user_id, None, None).await?;
    audit(&mut tx, None, Some(&user_id), "user.password_reset", Some(&user_id), json!({ "reset_id": reset_id }))
        .await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
