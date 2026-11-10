//! Connecting the Observer extension. The signed-in web app asks for a
//! one-time code and passes it to the extension, which trades it for its own
//! sign-in session. The password never reaches the extension.

use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Body, SessionView, session_view};
use crate::{
    auth::{Auth, audit, start_client_session},
    error::{AppError, AppResult},
    security::{hash_token, new_id, new_token, now},
    state::AppState,
};

const CODE_TTL_SECS: i64 = 5 * 60;

pub async fn create_code(State(state): State<AppState>, auth: Auth) -> AppResult<(StatusCode, Json<Value>)> {
    let code = new_token();
    let t = now();
    sqlx::query(
        "INSERT INTO extension_codes (id, code_hash, user_id, workspace_id, created_at, expires_at, sso) VALUES (?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(hash_token(&code))
    .bind(&auth.user_id)
    .bind(&auth.workspace_id)
    .bind(t)
    .bind(t + CODE_TTL_SECS)
    .bind(auth.sso)
    .execute(&state.db)
    .await?;
    Ok((StatusCode::CREATED, Json(json!({ "code": code, "expires_in": CODE_TTL_SECS }))))
}

#[derive(Deserialize)]
pub struct ExchangeBody {
    code: String,
}

pub async fn exchange(
    State(state): State<AppState>,
    headers: axum::http::HeaderMap,
    Body(body): Body<ExchangeBody>,
) -> AppResult<Json<SessionView>> {
    let mut tx = state.db.begin().await?;
    let row: Option<(String, String, String, bool)> = sqlx::query_as(
        "SELECT c.id, c.user_id, c.workspace_id, c.sso FROM extension_codes c
         JOIN memberships m ON m.user_id = c.user_id AND m.workspace_id = c.workspace_id
         WHERE c.code_hash = ? AND c.used_at IS NULL AND c.expires_at > ?",
    )
    .bind(hash_token(&body.code))
    .bind(now())
    .fetch_optional(&mut *tx)
    .await?;
    let (code_id, user_id, workspace_id, sso) = row
        .ok_or_else(|| AppError::bad_request("invalid_code", "This connect code has expired or was already used. Connect again from Peter."))?;
    sqlx::query("UPDATE extension_codes SET used_at = ? WHERE id = ?").bind(now()).bind(&code_id).execute(&mut *tx).await?;
    let tokens = start_client_session(&mut tx, &state, &user_id, &workspace_id, "extension", &crate::auth::user_agent(&headers)).await?;
    audit(&mut tx, Some(&workspace_id), Some(&user_id), "extension.connected", None, json!({})).await?;
    // An extension connected from a single sign-on session is one too.
    if sso {
        crate::auth::mark_sso(&mut tx, &tokens).await?;
    }
    let mut view = session_view(&mut tx, tokens, &user_id, &workspace_id).await?;
    if sso {
        (view.sso, view.mfa_setup_required) = (true, false);
    }
    tx.commit().await?;
    Ok(Json(view))
}
