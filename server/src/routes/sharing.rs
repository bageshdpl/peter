//! M8: sharing a session or document with a colleague, review requests, and
//! each person's notifications.

use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Body;
use crate::{
    auth::{Auth, audit},
    catalog,
    error::{AppError, AppResult},
    notify::{self, Note},
    security::{Role, new_id, now},
    state::AppState,
};

#[derive(Deserialize)]
pub struct ShareBody {
    user_id: String,
    step_id: Option<String>,
    #[serde(default)]
    review: bool,
    #[serde(default)]
    message: String,
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(session_id): Path<String>,
    Body(body): Body<ShareBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    let row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT validation_type, archived_at FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(&session_id)
            .bind(&auth.workspace_id)
            .fetch_optional(&state.db)
            .await?;
    let (kind, archived) = row.ok_or_else(|| AppError::not_found("No such session."))?;
    if archived.is_some() {
        return Err(AppError::conflict("archived", "This session is archived. Restore it first."));
    }
    if body.user_id == auth.user_id {
        return Err(AppError::bad_request("self", "Choose someone other than yourself."));
    }
    let member: Option<(String,)> = sqlx::query_as("SELECT role FROM memberships WHERE workspace_id = ? AND user_id = ?")
        .bind(&auth.workspace_id)
        .bind(&body.user_id)
        .fetch_optional(&state.db)
        .await?;
    let (role,) = member.ok_or_else(|| AppError::not_found("That person is not a member of this workspace."))?;
    let step_id = body.step_id.as_deref().map(str::trim).filter(|s| !s.is_empty());
    if let Some(step) = step_id
        && !catalog::find(&kind).is_some_and(|k| k.steps.iter().any(|s| s.id == step))
    {
        return Err(AppError::bad_request("invalid_step", "That step is not part of this session."));
    }
    if body.review {
        let Some(step) = step_id else {
            return Err(AppError::bad_request("no_document", "Choose the document to review."));
        };
        let has_doc: Option<(i64,)> = sqlx::query_as("SELECT current_version FROM documents WHERE session_id = ? AND step_id = ? AND current_version > 0")
            .bind(&session_id)
            .bind(step)
            .fetch_optional(&state.db)
            .await?;
        if has_doc.is_none() {
            return Err(AppError::bad_request("no_document", "Nothing has been drafted for this step yet."));
        }
        if role == "viewer" {
            return Err(AppError::bad_request("viewer", "Viewers cannot sign. Choose a member, admin or owner, or share it instead."));
        }
    }
    let message = super::projects::clean_text(&body.message, "message", 1000, false)?;
    let kind = if body.review { "review" } else { "share" };

    let id = new_id();
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO shares (id, workspace_id, session_id, step_id, user_id, shared_by, kind, message, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&session_id)
    .bind(step_id)
    .bind(&body.user_id)
    .bind(&auth.user_id)
    .bind(kind)
    .bind(&message)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    let who = notify::name_of(&mut tx, &auth.user_id).await?;
    let place = notify::place(&mut tx, &session_id, step_id).await?;
    let mut text = if body.review { format!("{who} asked you to review and sign {place}.") } else { format!("{who} shared {place} with you.") };
    if !message.is_empty() {
        text.push_str(&format!(" “{}”", message.chars().take(200).collect::<String>()));
    }
    notify::send(&mut tx, std::slice::from_ref(&body.user_id), &Note {
        workspace_id: &auth.workspace_id, actor_id: &auth.user_id, kind, session_id: &session_id, step_id, text,
    }).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), if body.review { "review.requested" } else { "session.shared" }, Some(&id),
        json!({ "session_id": session_id, "step_id": step_id, "user_id": body.user_id })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

#[derive(Serialize, sqlx::FromRow)]
struct ShareView {
    id: String,
    session_id: String,
    session_title: String,
    validation_type: String,
    project_name: Option<String>,
    step_id: Option<String>,
    user_id: String,
    user_name: String,
    shared_by: String,
    shared_by_name: String,
    kind: String,
    message: String,
    created_at: i64,
    done_at: Option<i64>,
}

const SHARE_SELECT: &str = "
    SELECT sh.id, sh.session_id, s.title AS session_title, s.validation_type, p.name AS project_name, sh.step_id,
           sh.user_id, u.name AS user_name, sh.shared_by, b.name AS shared_by_name, sh.kind, sh.message,
           sh.created_at, sh.done_at
    FROM shares sh
    JOIN validation_sessions s ON s.id = sh.session_id
    LEFT JOIN projects p ON p.id = s.project_id
    JOIN users u ON u.id = sh.user_id
    JOIN users b ON b.id = sh.shared_by";

fn with_labels(rows: Vec<ShareView>) -> Vec<Value> {
    rows.into_iter()
        .map(|r| {
            let label = r.step_id.as_deref().and_then(|id| {
                catalog::find(&r.validation_type).and_then(|k| k.steps.iter().find(|s| s.id == id)).map(|s| s.label)
            });
            let mut v = serde_json::to_value(&r).unwrap_or_default();
            v["step_label"] = json!(label);
            v
        })
        .collect()
}

/// Open shares and review requests on a session.
pub async fn list_for_session(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Json<Value>> {
    let rows: Vec<ShareView> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SHARE_SELECT} WHERE sh.session_id = ? AND sh.workspace_id = ? AND sh.revoked_at IS NULL ORDER BY sh.created_at DESC"
    )))
    .bind(&session_id)
    .bind(&auth.workspace_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "shares": with_labels(rows) })))
}

/// What colleagues have shared with me: open first, then the last 30 days of done.
pub async fn shared_with_me(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<ShareView> = sqlx::query_as(sqlx::AssertSqlSafe(format!(
        "{SHARE_SELECT} WHERE sh.workspace_id = ? AND sh.user_id = ? AND sh.revoked_at IS NULL AND s.archived_at IS NULL
           AND (sh.done_at IS NULL OR sh.done_at > ?)
         ORDER BY sh.done_at IS NOT NULL, sh.created_at DESC LIMIT 100"
    )))
    .bind(&auth.workspace_id)
    .bind(&auth.user_id)
    .bind(now() - 30 * 86_400)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "shares": with_labels(rows) })))
}

async fn find_share(state: &AppState, auth: &Auth, share_id: &str) -> AppResult<(String, String, String, Option<i64>)> {
    let row: Option<(String, String, String, Option<i64>)> = sqlx::query_as(
        "SELECT session_id, user_id, shared_by, done_at FROM shares WHERE id = ? AND workspace_id = ? AND revoked_at IS NULL",
    )
    .bind(share_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or_else(|| AppError::not_found("No such share."))
}

/// The person who shared it, or an admin, can take it back.
pub async fn revoke(State(state): State<AppState>, auth: Auth, Path(share_id): Path<String>) -> AppResult<StatusCode> {
    let (session_id, user_id, shared_by, _) = find_share(&state, &auth, &share_id).await?;
    if shared_by != auth.user_id && auth.role < Role::Admin {
        return Err(AppError::forbidden("Only the person who shared it, or an admin, can remove it."));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE shares SET revoked_at = ? WHERE id = ?").bind(now()).bind(&share_id).execute(&mut *tx).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "share.removed", Some(&share_id),
        json!({ "session_id": session_id, "user_id": user_id })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The person it was shared with marks it done (or open again).
#[derive(Deserialize)]
pub struct DoneBody {
    done: bool,
}

pub async fn set_done(State(state): State<AppState>, auth: Auth, Path(share_id): Path<String>, Body(body): Body<DoneBody>) -> AppResult<StatusCode> {
    let (_, user_id, ..) = find_share(&state, &auth, &share_id).await?;
    if user_id != auth.user_id {
        return Err(AppError::forbidden("Only the person it was shared with can mark it done."));
    }
    sqlx::query("UPDATE shares SET done_at = ? WHERE id = ?")
        .bind(body.done.then(now))
        .bind(&share_id)
        .execute(&state.db)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Notifications ----------

#[derive(Serialize, sqlx::FromRow)]
struct NotificationView {
    id: String,
    kind: String,
    session_id: Option<String>,
    step_id: Option<String>,
    text: String,
    actor_name: Option<String>,
    created_at: i64,
    read_at: Option<i64>,
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    unread: bool,
}

async fn unread_count(state: &AppState, auth: &Auth) -> AppResult<i64> {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM notifications WHERE workspace_id = ? AND user_id = ? AND read_at IS NULL")
        .bind(&auth.workspace_id)
        .bind(&auth.user_id)
        .fetch_one(&state.db)
        .await?;
    Ok(n)
}

pub async fn notifications(State(state): State<AppState>, auth: Auth, Query(q): Query<ListQuery>) -> AppResult<Json<Value>> {
    let items: Vec<NotificationView> = sqlx::query_as(
        "SELECT n.id, n.kind, n.session_id, n.step_id, n.text, a.name AS actor_name, n.created_at, n.read_at
         FROM notifications n LEFT JOIN users a ON a.id = n.actor_id
         WHERE n.workspace_id = ? AND n.user_id = ? AND (? = 0 OR n.read_at IS NULL)
         ORDER BY n.created_at DESC, n.rowid DESC LIMIT 100",
    )
    .bind(&auth.workspace_id)
    .bind(&auth.user_id)
    .bind(q.unread)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "notifications": items, "unread": unread_count(&state, &auth).await? })))
}

pub async fn unread(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    Ok(Json(json!({ "unread": unread_count(&state, &auth).await? })))
}

#[derive(Deserialize)]
pub struct ReadBody {
    #[serde(default)]
    ids: Vec<String>,
    #[serde(default)]
    all: bool,
}

pub async fn mark_read(State(state): State<AppState>, auth: Auth, Body(body): Body<ReadBody>) -> AppResult<Json<Value>> {
    let t = now();
    if body.all {
        sqlx::query("UPDATE notifications SET read_at = ? WHERE workspace_id = ? AND user_id = ? AND read_at IS NULL")
            .bind(t)
            .bind(&auth.workspace_id)
            .bind(&auth.user_id)
            .execute(&state.db)
            .await?;
    } else {
        for id in body.ids.iter().take(200) {
            sqlx::query("UPDATE notifications SET read_at = ? WHERE id = ? AND workspace_id = ? AND user_id = ? AND read_at IS NULL")
                .bind(t)
                .bind(id)
                .bind(&auth.workspace_id)
                .bind(&auth.user_id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(Json(json!({ "unread": unread_count(&state, &auth).await? })))
}
