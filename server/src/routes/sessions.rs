use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{
    Body,
    projects::{clean_text, project_in_workspace},
};
use crate::{
    auth::{Auth, audit},
    catalog,
    error::{AppError, AppResult},
    security::{Role, new_id, now},
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct SessionView {
    id: String,
    project_id: Option<String>,
    project_name: Option<String>,
    title: String,
    validation_type: String,
    current_step: Option<String>,
    baseline_session_id: Option<String>,
    baseline_title: Option<String>,
    created_by_name: String,
    created_at: i64,
    updated_at: i64,
    archived_at: Option<i64>,
}

const SESSION_SELECT: &str = "
    SELECT s.id, s.project_id, p.name AS project_name, s.title, s.validation_type, s.current_step,
           s.baseline_session_id, b.title AS baseline_title,
           u.name AS created_by_name, s.created_at, s.updated_at, s.archived_at
    FROM validation_sessions s
    JOIN users u ON u.id = s.created_by
    LEFT JOIN projects p ON p.id = s.project_id
    LEFT JOIN validation_sessions b ON b.id = s.baseline_session_id";

pub async fn types() -> Json<Value> {
    Json(json!({ "types": catalog::TYPES }))
}

#[derive(Deserialize)]
pub struct ListQuery {
    project_id: Option<String>,
    /// Only sessions not filed in any project.
    #[serde(default)]
    unfiled: bool,
    #[serde(default)]
    archived: bool,
}

pub async fn list(State(state): State<AppState>, auth: Auth, Query(q): Query<ListQuery>) -> AppResult<Json<Value>> {
    let sql = format!(
        "{SESSION_SELECT}
         WHERE s.workspace_id = ? AND (s.archived_at IS NOT NULL) = ?
           AND (? IS NULL OR s.project_id = ?)
           AND (? = 0 OR s.project_id IS NULL)
         ORDER BY s.updated_at DESC"
    );
    let sessions: Vec<SessionView> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&auth.workspace_id)
        .bind(q.archived)
        .bind(&q.project_id)
        .bind(&q.project_id)
        .bind(q.unfiled)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!({ "sessions": sessions })))
}

async fn load(state: &AppState, auth: &Auth, id: &str) -> AppResult<SessionView> {
    let sql = format!("{SESSION_SELECT} WHERE s.id = ? AND s.workspace_id = ?");
    let session: Option<SessionView> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(id).bind(&auth.workspace_id).fetch_optional(&state.db).await?;
    session.ok_or_else(|| AppError::not_found("No such session."))
}

#[derive(Serialize, sqlx::FromRow)]
struct StepState {
    step_id: String,
    status: String,
    updated_by_name: String,
    updated_at: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct DocumentSummary {
    step_id: String,
    title: String,
    status: String,
    current_version: i64,
    updated_at: i64,
}

/// A session with its validation type, the state of each step, and its documents.
pub async fn get(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<Value>> {
    let session = load(&state, &auth, &id).await?;
    let kind = catalog::find(&session.validation_type);
    let steps: Vec<StepState> = sqlx::query_as(
        "SELECT s.step_id, s.status, u.name AS updated_by_name, s.updated_at
         FROM step_states s JOIN users u ON u.id = s.updated_by WHERE s.session_id = ?",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    let documents: Vec<DocumentSummary> = sqlx::query_as(
        "SELECT step_id, title, status, current_version, updated_at FROM documents WHERE session_id = ?",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "session": session, "type": kind, "step_states": steps, "documents": documents })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    title: String,
    validation_type: String,
    project_id: Option<String>,
    /// An earlier session of the same system that this one builds on.
    baseline_session_id: Option<String>,
}

/// Checks that a session can serve as the baseline of another: same
/// workspace, not the session itself, and the same project when both are filed.
async fn check_baseline(
    conn: &mut sqlx::SqliteConnection,
    workspace_id: &str,
    session_id: Option<&str>,
    project_id: Option<&str>,
    baseline_id: &str,
) -> AppResult<()> {
    if Some(baseline_id) == session_id {
        return Err(AppError::bad_request("invalid_baseline", "A session cannot build on itself."));
    }
    let row: Option<(Option<String>,)> =
        sqlx::query_as("SELECT project_id FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(baseline_id)
            .bind(workspace_id)
            .fetch_optional(&mut *conn)
            .await?;
    let (baseline_project,) = row.ok_or_else(|| AppError::bad_request("invalid_baseline", "That earlier session does not exist."))?;
    if let (Some(p), Some(b)) = (project_id, baseline_project.as_deref())
        && p != b
    {
        return Err(AppError::bad_request("invalid_baseline", "The earlier validation must belong to the same project."));
    }
    Ok(())
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<CreateBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    let title = clean_text(&body.title, "session title", 160, true)?;
    let kind = catalog::find(&body.validation_type)
        .ok_or_else(|| AppError::bad_request("unknown_type", "Pick a validation type from the list."))?;
    if !kind.available {
        return Err(AppError::bad_request("type_unavailable", format!("{} is not available yet.", kind.title)));
    }

    let mut tx = state.db.begin().await?;
    if let Some(project_id) = &body.project_id
        && project_in_workspace(&mut tx, &auth.workspace_id, project_id).await?
    {
        return Err(AppError::conflict("archived", "Restore this project before adding sessions to it."));
    }
    if let Some(baseline) = &body.baseline_session_id {
        check_baseline(&mut tx, &auth.workspace_id, None, body.project_id.as_deref(), baseline).await?;
    }
    let (id, t) = (new_id(), now());
    sqlx::query(
        "INSERT INTO validation_sessions
         (id, workspace_id, project_id, title, validation_type, current_step, baseline_session_id, created_by, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&body.project_id)
    .bind(&title)
    .bind(kind.id)
    .bind(kind.steps.first().map(|s| s.id))
    .bind(&body.baseline_session_id)
    .bind(&auth.user_id)
    .bind(t)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    if let Some(project_id) = &body.project_id {
        sqlx::query("UPDATE projects SET updated_at = ? WHERE id = ?").bind(t).bind(project_id).execute(&mut *tx).await?;
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "session.created", Some(&id),
        json!({ "title": title, "validation_type": kind.id, "project_id": body.project_id,
                "baseline_session_id": body.baseline_session_id })).await?;
    tx.commit().await?;
    let Json(view) = get(State(state), auth, Path(id)).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

/// `project_id` moves the session: a project id files it there, `null` unfiles it.
/// `baseline_session_id` likewise sets or clears the earlier validation.
#[derive(Deserialize)]
pub struct UpdateBody {
    title: Option<String>,
    #[serde(default, deserialize_with = "present")]
    project_id: Option<Option<String>>,
    #[serde(default, deserialize_with = "present")]
    baseline_session_id: Option<Option<String>>,
}

/// Distinguishes a field set to `null` from a field left out.
fn present<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Option<String>>, D::Error> {
    Option::<String>::deserialize(d).map(Some)
}

pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Body(body): Body<UpdateBody>,
) -> AppResult<Json<Value>> {
    auth.require(Role::Member)?;
    let current = load(&state, &auth, &id).await?;
    if current.archived_at.is_some() {
        return Err(AppError::conflict("archived", "Restore this session before changing it."));
    }
    let title = match &body.title { Some(v) => clean_text(v, "session title", 160, true)?, None => current.title.clone() };
    let project_id = body.project_id.clone().unwrap_or(current.project_id.clone());
    let baseline = body.baseline_session_id.clone().unwrap_or(current.baseline_session_id.clone());

    let mut tx = state.db.begin().await?;
    if let Some(pid) = &project_id
        && Some(pid) != current.project_id.as_ref()
        && project_in_workspace(&mut tx, &auth.workspace_id, pid).await?
    {
        return Err(AppError::conflict("archived", "That project is archived."));
    }
    if let Some(b) = &baseline
        && (Some(b) != current.baseline_session_id.as_ref() || project_id != current.project_id)
    {
        check_baseline(&mut tx, &auth.workspace_id, Some(&id), project_id.as_deref(), b).await?;
    }
    let mut changes = serde_json::Map::new();
    if title != current.title {
        changes.insert("title".into(), json!({ "from": current.title, "to": title }));
    }
    if project_id != current.project_id {
        changes.insert("project_id".into(), json!({ "from": current.project_id, "to": project_id }));
    }
    if baseline != current.baseline_session_id {
        changes.insert("baseline_session_id".into(), json!({ "from": current.baseline_session_id, "to": baseline }));
    }
    if !changes.is_empty() {
        let t = now();
        sqlx::query("UPDATE validation_sessions SET title = ?, project_id = ?, baseline_session_id = ?, updated_at = ? WHERE id = ?")
            .bind(&title)
            .bind(&project_id)
            .bind(&baseline)
            .bind(t)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "session.updated", Some(&id),
            Value::Object(changes)).await?;
    }
    tx.commit().await?;
    get(State(state), auth, Path(id)).await
}

async fn set_archived(state: AppState, auth: Auth, id: String, archive: bool) -> AppResult<Json<Value>> {
    auth.require(Role::Member)?;
    let current = load(&state, &auth, &id).await?;
    if current.archived_at.is_some() != archive {
        let mut tx = state.db.begin().await?;
        let t = now();
        sqlx::query("UPDATE validation_sessions SET archived_at = ?, updated_at = ? WHERE id = ?")
            .bind(if archive { Some(t) } else { None })
            .bind(t)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id),
            if archive { "session.archived" } else { "session.restored" }, Some(&id), json!({})).await?;
        tx.commit().await?;
    }
    get(State(state), auth, Path(id)).await
}

pub async fn archive(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<Value>> {
    set_archived(state, auth, id, true).await
}

pub async fn restore(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<Value>> {
    set_archived(state, auth, id, false).await
}
