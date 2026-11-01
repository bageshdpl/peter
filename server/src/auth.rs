//! Sessions, the authenticated-request extractor, and the audit trail.

use axum::{
    extract::{FromRequestParts, OriginalUri},
    http::{HeaderMap, StatusCode, header, request::Parts},
};
use serde::Serialize;
use serde_json::Value;
use sqlx::{Sqlite, SqliteConnection, Transaction};

use crate::{
    error::{AppError, AppResult},
    security::{Role, hash_token, new_id, new_token, now},
    state::AppState,
};

/// The caller of an authenticated request, in the workspace their token is for.
#[derive(Debug, Clone)]
pub struct Auth {
    pub session_id: String,
    pub user_id: String,
    pub workspace_id: String,
    pub role: Role,
}

impl Auth {
    pub fn require(&self, min: Role) -> AppResult<()> {
        if self.role >= min {
            Ok(())
        } else {
            Err(AppError::forbidden(format!("This needs the {} role or higher.", min.as_str())))
        }
    }
}

impl FromRequestParts<AppState> for Auth {
    type Rejection = AppError;

    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("Sign in to continue."))?;
        let token = header
            .strip_prefix("Bearer ")
            .ok_or_else(|| AppError::unauthorized("Use a Bearer token."))?;

        // The membership join means a removed member loses access at once.
        let row: Option<(String, String, String, String, bool, bool)> = sqlx::query_as(
            "SELECT s.id, s.user_id, s.workspace_id, m.role, w.require_mfa, (um.enabled_at IS NOT NULL)
             FROM sessions s
             JOIN memberships m ON m.workspace_id = s.workspace_id AND m.user_id = s.user_id
             JOIN workspaces w ON w.id = s.workspace_id
             LEFT JOIN user_mfa um ON um.user_id = s.user_id
             WHERE s.access_hash = ? AND s.revoked_at IS NULL AND s.access_expires_at > ?",
        )
        .bind(hash_token(token))
        .bind(now())
        .fetch_optional(&state.db)
        .await?;

        let (session_id, user_id, workspace_id, role, require_mfa, has_mfa) =
            row.ok_or_else(|| AppError::unauthorized("Your sign-in has expired. Sign in again."))?;

        // A workspace that requires two-factor sign-in lets people without it
        // do nothing except set it up (or leave, or switch workspace).
        if require_mfa && !has_mfa {
            let path = parts.extensions.get::<OriginalUri>().map(|u| u.path().to_string()).unwrap_or_else(|| parts.uri.path().to_string());
            let allowed = ["/v1/auth/me", "/v1/auth/logout", "/v1/auth/mfa", "/v1/auth/mfa/setup", "/v1/auth/mfa/enable",
                "/v1/auth/workspaces", "/v1/auth/switch", "/v1/auth/sessions"];
            if !allowed.contains(&path.as_str()) {
                return Err(AppError::new(
                    StatusCode::FORBIDDEN,
                    "mfa_setup_required",
                    "This workspace requires two-factor sign-in. Set it up in Settings to continue.",
                ));
            }
        }
        Ok(Auth { session_id, user_id, workspace_id, role: Role::parse(&role)? })
    }
}

#[derive(Debug, Serialize)]
pub struct Tokens {
    pub access_token: String,
    pub refresh_token: String,
    pub expires_in: i64,
}

/// The browser or app making a request, as it describes itself (for the
/// list of active sessions).
pub fn user_agent(headers: &HeaderMap) -> String {
    headers.get(header::USER_AGENT).and_then(|v| v.to_str().ok()).unwrap_or("").chars().take(300).collect()
}

/// Starts a web session for a user in a workspace they belong to.
pub async fn start_session(
    conn: &mut SqliteConnection,
    state: &AppState,
    user_id: &str,
    workspace_id: &str,
    user_agent: &str,
) -> AppResult<Tokens> {
    start_client_session(conn, state, user_id, workspace_id, "web", user_agent).await
}

/// Starts a session for a particular client (`web` or `extension`).
pub async fn start_client_session(
    conn: &mut SqliteConnection,
    state: &AppState,
    user_id: &str,
    workspace_id: &str,
    client: &str,
    user_agent: &str,
) -> AppResult<Tokens> {
    let (access, refresh) = (new_token(), new_token());
    let t = now();
    sqlx::query(
        "INSERT INTO sessions
         (id, user_id, workspace_id, access_hash, access_expires_at, refresh_hash, refresh_expires_at, created_at, client,
          user_agent, last_used_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(user_id)
    .bind(workspace_id)
    .bind(hash_token(&access))
    .bind(t + state.config.access_ttl_secs)
    .bind(hash_token(&refresh))
    .bind(t + state.config.refresh_ttl_secs)
    .bind(t)
    .bind(client)
    .bind(user_agent)
    .bind(t)
    .execute(&mut *conn)
    .await?;
    Ok(Tokens { access_token: access, refresh_token: refresh, expires_in: state.config.access_ttl_secs })
}

/// Issues new tokens for an existing session and invalidates the old ones.
pub async fn rotate_session(conn: &mut SqliteConnection, state: &AppState, session_id: &str) -> AppResult<Tokens> {
    let (access, refresh) = (new_token(), new_token());
    let t = now();
    sqlx::query(
        "UPDATE sessions SET access_hash = ?, access_expires_at = ?, refresh_hash = ?, refresh_expires_at = ?,
         last_used_at = ? WHERE id = ?",
    )
    .bind(hash_token(&access))
    .bind(t + state.config.access_ttl_secs)
    .bind(hash_token(&refresh))
    .bind(t + state.config.refresh_ttl_secs)
    .bind(t)
    .bind(session_id)
    .execute(&mut *conn)
    .await?;
    Ok(Tokens { access_token: access, refresh_token: refresh, expires_in: state.config.access_ttl_secs })
}

pub async fn revoke_user_sessions(
    conn: &mut SqliteConnection,
    user_id: &str,
    workspace_id: Option<&str>,
    except_session: Option<&str>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE sessions SET revoked_at = ?
         WHERE user_id = ? AND revoked_at IS NULL
           AND (? IS NULL OR workspace_id = ?)
           AND (? IS NULL OR id <> ?)",
    )
    .bind(now())
    .bind(user_id)
    .bind(workspace_id)
    .bind(workspace_id)
    .bind(except_session)
    .bind(except_session)
    .execute(&mut *conn)
    .await?;
    Ok(())
}

/// Records an event in the append-only audit trail.
pub async fn audit(
    tx: &mut Transaction<'_, Sqlite>,
    workspace_id: Option<&str>,
    actor_user_id: Option<&str>,
    action: &str,
    target: Option<&str>,
    detail: Value,
) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO audit_events (id, workspace_id, actor_user_id, action, target, detail, at)
         VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(workspace_id)
    .bind(actor_user_id)
    .bind(action)
    .bind(target)
    .bind(detail.to_string())
    .bind(now())
    .execute(&mut **tx)
    .await?;
    Ok(())
}
