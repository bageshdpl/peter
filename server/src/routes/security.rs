//! Two-factor sign-in settings, active sessions, and the workspace's
//! two-factor requirement.

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Body, auth::verify_password};
use crate::{
    auth::{Auth, audit},
    error::{AppError, AppResult},
    mfa,
    security::{Role, hash_token, new_id, now},
    state::AppState,
    throttle,
};

async fn email_of(state: &AppState, user_id: &str) -> AppResult<String> {
    let (email,): (String,) = sqlx::query_as("SELECT email FROM users WHERE id = ?").bind(user_id).fetch_one(&state.db).await?;
    Ok(email)
}

/// Stores fresh recovery codes (replacing any old ones) and returns them.
async fn new_recovery_codes(conn: &mut sqlx::SqliteConnection, user_id: &str) -> AppResult<Vec<String>> {
    let codes = mfa::new_recovery_codes();
    sqlx::query("DELETE FROM mfa_recovery_codes WHERE user_id = ?").bind(user_id).execute(&mut *conn).await?;
    for code in &codes {
        sqlx::query("INSERT INTO mfa_recovery_codes (id, user_id, code_hash) VALUES (?, ?, ?)")
            .bind(new_id())
            .bind(user_id)
            .bind(hash_token(&mfa::normalize_recovery(code)))
            .execute(&mut *conn)
            .await?;
    }
    Ok(codes)
}

/// Checks a current authenticator code for a signed-in person, counting
/// wrong codes towards their lock.
async fn check_code(state: &AppState, user_id: &str, code: &str, pending: bool) -> AppResult<()> {
    let key = throttle::user_key(user_id);
    throttle::check(&state.db, &key).await?;
    let sql = if pending {
        "SELECT secret, last_step FROM user_mfa WHERE user_id = ? AND enabled_at IS NULL"
    } else {
        "SELECT secret, last_step FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL"
    };
    let row: Option<(String, i64)> = sqlx::query_as(sql).bind(user_id).fetch_optional(&state.db).await?;
    let (secret, last) = row.ok_or_else(|| AppError::conflict("no_mfa", "Start two-factor setup first."))?;
    match mfa::verify(&secret, code, now(), last) {
        Some(step) => {
            sqlx::query("UPDATE user_mfa SET last_step = ? WHERE user_id = ?").bind(step).bind(user_id).execute(&state.db).await?;
            throttle::clear(&state.db, &key).await?;
            Ok(())
        }
        None => {
            let wrong = AppError::bad_request("wrong_code", "That code is not right. Check the time on your phone and try the newest code.");
            Err(throttle::fail(&state.db, &key).await?.unwrap_or(wrong))
        }
    }
}

/// Two-factor settings protect the person's password sign-in everywhere, so a
/// session vouched for by one workspace's identity provider may not change them.
fn not_from_sso(auth: &Auth) -> AppResult<()> {
    if auth.sso {
        return Err(AppError::new(
            StatusCode::FORBIDDEN,
            "sso_session",
            "You signed in with single sign-on, which handles two-factor sign-in. To change Peter's own, sign in with your password.",
        ));
    }
    Ok(())
}

pub async fn status(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT enabled_at FROM user_mfa WHERE user_id = ?").bind(&auth.user_id).fetch_optional(&state.db).await?;
    let enabled_at = row.and_then(|(e,)| e);
    let (left,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mfa_recovery_codes WHERE user_id = ? AND used_at IS NULL")
        .bind(&auth.user_id)
        .fetch_one(&state.db)
        .await?;
    let (required,): (bool,) =
        sqlx::query_as("SELECT require_mfa FROM workspaces WHERE id = ?").bind(&auth.workspace_id).fetch_one(&state.db).await?;
    Ok(Json(json!({
        "enabled": enabled_at.is_some(), "enabled_at": enabled_at, "recovery_codes_left": left, "workspace_requires": required,
    })))
}

/// Starts setup: a new secret for the authenticator app. Not active until confirmed.
pub async fn setup(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    not_from_sso(&auth)?;
    let enabled: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL")
        .bind(&auth.user_id)
        .fetch_optional(&state.db)
        .await?;
    if enabled.is_some() {
        return Err(AppError::conflict("already_enabled", "Two-factor sign-in is already on."));
    }
    let secret = mfa::new_secret();
    sqlx::query(
        "INSERT INTO user_mfa (user_id, secret, created_at) VALUES (?, ?, ?)
         ON CONFLICT (user_id) DO UPDATE SET secret = excluded.secret, last_step = 0, created_at = excluded.created_at",
    )
    .bind(&auth.user_id)
    .bind(&secret)
    .bind(now())
    .execute(&state.db)
    .await?;
    let email = email_of(&state, &auth.user_id).await?;
    Ok(Json(json!({ "secret": secret, "otpauth_url": mfa::otpauth_url(&secret, &email) })))
}

#[derive(Deserialize)]
pub struct CodeBody {
    code: String,
}

/// Confirms setup with a code from the app. Returns recovery codes, shown once.
pub async fn enable(State(state): State<AppState>, auth: Auth, Body(body): Body<CodeBody>) -> AppResult<Json<Value>> {
    not_from_sso(&auth)?;
    check_code(&state, &auth.user_id, &body.code, true).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE user_mfa SET enabled_at = ? WHERE user_id = ?").bind(now()).bind(&auth.user_id).execute(&mut *tx).await?;
    let codes = new_recovery_codes(&mut tx, &auth.user_id).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "user.mfa_enabled", Some(&auth.user_id), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "recovery_codes": codes })))
}

#[derive(Deserialize)]
pub struct DisableBody {
    password: String,
    code: String,
}

pub async fn disable(State(state): State<AppState>, auth: Auth, Body(body): Body<DisableBody>) -> AppResult<StatusCode> {
    not_from_sso(&auth)?;
    let key = throttle::user_key(&auth.user_id);
    throttle::check(&state.db, &key).await?;
    let (hash,): (String,) = sqlx::query_as("SELECT password_hash FROM users WHERE id = ?").bind(&auth.user_id).fetch_one(&state.db).await?;
    if !verify_password(body.password, hash).await? {
        let wrong = AppError::bad_request("wrong_password", "That password is not right.");
        return Err(throttle::fail(&state.db, &key).await?.unwrap_or(wrong));
    }
    check_code(&state, &auth.user_id, &body.code, false).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM user_mfa WHERE user_id = ?").bind(&auth.user_id).execute(&mut *tx).await?;
    sqlx::query("DELETE FROM mfa_recovery_codes WHERE user_id = ?").bind(&auth.user_id).execute(&mut *tx).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "user.mfa_disabled", Some(&auth.user_id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// New recovery codes, replacing the old ones. Needs a current app code.
pub async fn regenerate_codes(State(state): State<AppState>, auth: Auth, Body(body): Body<CodeBody>) -> AppResult<Json<Value>> {
    not_from_sso(&auth)?;
    check_code(&state, &auth.user_id, &body.code, false).await?;
    let mut tx = state.db.begin().await?;
    let codes = new_recovery_codes(&mut tx, &auth.user_id).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "user.recovery_codes_renewed", Some(&auth.user_id), json!({})).await?;
    tx.commit().await?;
    Ok(Json(json!({ "recovery_codes": codes })))
}

// ---------- Active sessions ----------

#[derive(Serialize, sqlx::FromRow)]
struct SessionRow {
    id: String,
    client: String,
    user_agent: String,
    workspace_name: String,
    created_at: i64,
    last_used_at: Option<i64>,
}

/// A single sign-on session sees and ends sessions in its own workspace only.
fn only_workspace(auth: &Auth) -> Option<&str> {
    auth.sso.then_some(auth.workspace_id.as_str())
}

pub async fn list_sessions(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<SessionRow> = sqlx::query_as(
        "SELECT s.id, s.client, s.user_agent, w.name AS workspace_name, s.created_at, s.last_used_at
         FROM sessions s JOIN workspaces w ON w.id = s.workspace_id
         WHERE s.user_id = ? AND s.revoked_at IS NULL AND s.refresh_expires_at > ? AND (? IS NULL OR s.workspace_id = ?)
         ORDER BY COALESCE(s.last_used_at, s.created_at) DESC",
    )
    .bind(&auth.user_id)
    .bind(now())
    .bind(only_workspace(&auth))
    .bind(only_workspace(&auth))
    .fetch_all(&state.db)
    .await?;
    let sessions: Vec<Value> = rows
        .into_iter()
        .map(|r| {
            let current = r.id == auth.session_id;
            let mut v = serde_json::to_value(r).unwrap_or(Value::Null);
            v["current"] = Value::Bool(current);
            v
        })
        .collect();
    Ok(Json(json!({ "sessions": sessions })))
}

pub async fn revoke_session(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<StatusCode> {
    let mut tx = state.db.begin().await?;
    let done = sqlx::query("UPDATE sessions SET revoked_at = ? WHERE id = ? AND user_id = ? AND revoked_at IS NULL AND (? IS NULL OR workspace_id = ?)")
        .bind(now())
        .bind(&id)
        .bind(&auth.user_id)
        .bind(only_workspace(&auth))
        .bind(only_workspace(&auth))
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("No such active session."));
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "auth.session_revoked", Some(&id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

pub async fn revoke_other_sessions(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let mut tx = state.db.begin().await?;
    let done = sqlx::query("UPDATE sessions SET revoked_at = ? WHERE user_id = ? AND id <> ? AND revoked_at IS NULL AND (? IS NULL OR workspace_id = ?)")
        .bind(now())
        .bind(&auth.user_id)
        .bind(&auth.session_id)
        .bind(only_workspace(&auth))
        .bind(only_workspace(&auth))
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "auth.other_sessions_revoked", None,
        json!({ "count": done.rows_affected() })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "revoked": done.rows_affected() })))
}

// ---------- Workspace policy ----------

#[derive(Deserialize)]
pub struct PolicyBody {
    require_mfa: bool,
}

pub async fn set_policy(State(state): State<AppState>, auth: Auth, Body(body): Body<PolicyBody>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    if body.require_mfa {
        // The admin turning this on must not lock themselves out.
        let has: Option<(i64,)> = sqlx::query_as("SELECT 1 FROM user_mfa WHERE user_id = ? AND enabled_at IS NOT NULL")
            .bind(&auth.user_id)
            .fetch_optional(&state.db)
            .await?;
        if has.is_none() {
            return Err(AppError::conflict("mfa_needed", "Turn on two-factor sign-in for yourself before requiring it."));
        }
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE workspaces SET require_mfa = ? WHERE id = ?").bind(body.require_mfa).bind(&auth.workspace_id).execute(&mut *tx).await?;
    let (without,): (i64,) = sqlx::query_as(
        "SELECT COUNT(*) FROM memberships m LEFT JOIN user_mfa um ON um.user_id = m.user_id AND um.enabled_at IS NOT NULL
         WHERE m.workspace_id = ? AND um.user_id IS NULL",
    )
    .bind(&auth.workspace_id)
    .fetch_one(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "workspace.mfa_policy_changed", Some(&auth.workspace_id),
        json!({ "require_mfa": body.require_mfa, "members_without_mfa": without })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "require_mfa": body.require_mfa, "members_without_mfa": without })))
}
