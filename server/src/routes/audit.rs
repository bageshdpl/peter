use axum::{
    Json,
    extract::{Query, State},
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{auth::Auth, error::AppResult, security::Role, state::AppState};

#[derive(Deserialize)]
pub struct AuditQuery {
    /// Return events older than this sequence number (for paging back).
    before: Option<i64>,
    limit: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct AuditEvent {
    seq: i64,
    id: String,
    actor_user_id: Option<String>,
    actor_email: Option<String>,
    action: String,
    target: Option<String>,
    detail: String,
    at: i64,
}

/// The workspace's audit trail, newest first. Admins and owners only.
pub async fn list(State(state): State<AppState>, auth: Auth, Query(q): Query<AuditQuery>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let limit = q.limit.unwrap_or(50).clamp(1, 200);
    let rows: Vec<AuditEvent> = sqlx::query_as(
        "SELECT a.seq, a.id, a.actor_user_id, u.email AS actor_email, a.action, a.target, a.detail, a.at
         FROM audit_events a LEFT JOIN users u ON u.id = a.actor_user_id
         WHERE a.workspace_id = ? AND (? IS NULL OR a.seq < ?)
         ORDER BY a.seq DESC LIMIT ?",
    )
    .bind(&auth.workspace_id)
    .bind(q.before)
    .bind(q.before)
    .bind(limit + 1)
    .fetch_all(&state.db)
    .await?;

    let has_more = rows.len() as i64 > limit;
    let events: Vec<Value> = rows
        .into_iter()
        .take(limit as usize)
        .map(|e| {
            let detail: Value = serde_json::from_str(&e.detail).unwrap_or(Value::Null);
            json!({
                "seq": e.seq, "id": e.id, "actor_user_id": e.actor_user_id, "actor_email": e.actor_email,
                "action": e.action, "target": e.target, "detail": detail, "at": e.at,
            })
        })
        .collect();
    let next_before = if has_more { events.last().and_then(|e| e["seq"].as_i64()) } else { None };
    Ok(Json(json!({ "events": events, "next_before": next_before })))
}
