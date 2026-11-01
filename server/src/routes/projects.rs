use axum::{
    Json,
    extract::{Path, Query, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sqlx::SqliteConnection;

use super::Body;
use crate::{
    auth::{Auth, audit},
    error::{AppError, AppResult},
    security::{Role, new_id, now},
    state::AppState,
};

#[derive(Serialize, sqlx::FromRow)]
pub struct ProjectView {
    id: String,
    name: String,
    system_name: String,
    vendor: String,
    description: String,
    created_by_name: String,
    created_at: i64,
    updated_at: i64,
    archived_at: Option<i64>,
    session_count: i64,
    source_count: i64,
}

const PROJECT_SELECT: &str = "
    SELECT p.id, p.name, p.system_name, p.vendor, p.description, u.name AS created_by_name,
           p.created_at, p.updated_at, p.archived_at,
           (SELECT COUNT(*) FROM validation_sessions s WHERE s.project_id = p.id AND s.archived_at IS NULL) AS session_count,
           (SELECT COUNT(*) FROM project_sources f WHERE f.project_id = p.id AND f.deleted_at IS NULL) AS source_count
    FROM projects p JOIN users u ON u.id = p.created_by";

/// Finds a project in the caller's workspace. Returns whether it is archived.
pub async fn project_in_workspace(conn: &mut SqliteConnection, workspace_id: &str, project_id: &str) -> AppResult<bool> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT archived_at FROM projects WHERE id = ? AND workspace_id = ?")
            .bind(project_id)
            .bind(workspace_id)
            .fetch_optional(&mut *conn)
            .await?;
    row.map(|(archived,)| archived.is_some()).ok_or_else(|| AppError::not_found("No such project."))
}

pub fn clean_text(value: &str, field: &str, max: usize, required: bool) -> AppResult<String> {
    let value = value.trim();
    if (required && value.is_empty()) || value.chars().count() > max {
        let need = if required { format!("1 to {max}") } else { format!("at most {max}") };
        return Err(AppError::bad_request("invalid_field", format!("The {field} must be {need} characters.")));
    }
    Ok(value.to_string())
}

#[derive(Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    archived: bool,
}

pub async fn list(State(state): State<AppState>, auth: Auth, Query(q): Query<ListQuery>) -> AppResult<Json<Value>> {
    let sql = format!(
        "{PROJECT_SELECT} WHERE p.workspace_id = ? AND (p.archived_at IS NOT NULL) = ?
         ORDER BY p.updated_at DESC"
    );
    let projects: Vec<ProjectView> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(&auth.workspace_id).bind(q.archived).fetch_all(&state.db).await?;
    Ok(Json(json!({ "projects": projects })))
}

pub async fn get(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<ProjectView>> {
    let sql = format!("{PROJECT_SELECT} WHERE p.id = ? AND p.workspace_id = ?");
    let project: Option<ProjectView> =
        sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(&id).bind(&auth.workspace_id).fetch_optional(&state.db).await?;
    project.map(Json).ok_or_else(|| AppError::not_found("No such project."))
}

#[derive(Deserialize)]
pub struct CreateBody {
    name: String,
    #[serde(default)]
    system_name: String,
    #[serde(default)]
    vendor: String,
    #[serde(default)]
    description: String,
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<CreateBody>,
) -> AppResult<(StatusCode, Json<ProjectView>)> {
    auth.require(Role::Member)?;
    let name = clean_text(&body.name, "project name", 120, true)?;
    let system_name = clean_text(&body.system_name, "system name", 120, false)?;
    let vendor = clean_text(&body.vendor, "vendor", 120, false)?;
    let description = clean_text(&body.description, "description", 4000, false)?;

    let (id, t) = (new_id(), now());
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO projects (id, workspace_id, name, system_name, vendor, description, created_by, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&name)
    .bind(&system_name)
    .bind(&vendor)
    .bind(&description)
    .bind(&auth.user_id)
    .bind(t)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "project.created", Some(&id),
        json!({ "name": name, "system_name": system_name })).await?;
    tx.commit().await?;
    let Json(view) = get(State(state), auth, Path(id)).await?;
    Ok((StatusCode::CREATED, Json(view)))
}

#[derive(Deserialize)]
pub struct UpdateBody {
    name: Option<String>,
    system_name: Option<String>,
    vendor: Option<String>,
    description: Option<String>,
}

pub async fn update(
    State(state): State<AppState>,
    auth: Auth,
    Path(id): Path<String>,
    Body(body): Body<UpdateBody>,
) -> AppResult<Json<ProjectView>> {
    auth.require(Role::Member)?;
    let mut tx = state.db.begin().await?;
    if project_in_workspace(&mut tx, &auth.workspace_id, &id).await? {
        return Err(AppError::conflict("archived", "Restore this project before changing it."));
    }
    let (old_name, old_system, old_vendor, old_desc): (String, String, String, String) =
        sqlx::query_as("SELECT name, system_name, vendor, description FROM projects WHERE id = ?")
            .bind(&id)
            .fetch_one(&mut *tx)
            .await?;
    let name = match body.name { Some(v) => clean_text(&v, "project name", 120, true)?, None => old_name.clone() };
    let system_name = match body.system_name { Some(v) => clean_text(&v, "system name", 120, false)?, None => old_system.clone() };
    let vendor = match body.vendor { Some(v) => clean_text(&v, "vendor", 120, false)?, None => old_vendor.clone() };
    let description = match body.description { Some(v) => clean_text(&v, "description", 4000, false)?, None => old_desc.clone() };

    // Record only the fields that changed, with before and after.
    let mut changes = serde_json::Map::new();
    for (field, before, after) in [
        ("name", &old_name, &name),
        ("system_name", &old_system, &system_name),
        ("vendor", &old_vendor, &vendor),
        ("description", &old_desc, &description),
    ] {
        if before != after {
            changes.insert(field.into(), json!({ "from": before, "to": after }));
        }
    }
    if !changes.is_empty() {
        sqlx::query("UPDATE projects SET name = ?, system_name = ?, vendor = ?, description = ?, updated_at = ? WHERE id = ?")
            .bind(&name)
            .bind(&system_name)
            .bind(&vendor)
            .bind(&description)
            .bind(now())
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "project.updated", Some(&id),
            Value::Object(changes)).await?;
    }
    tx.commit().await?;
    get(State(state), auth, Path(id)).await
}

async fn set_archived(state: AppState, auth: Auth, id: String, archive: bool) -> AppResult<Json<ProjectView>> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let archived = project_in_workspace(&mut tx, &auth.workspace_id, &id).await?;
    if archived != archive {
        let t = now();
        sqlx::query("UPDATE projects SET archived_at = ?, updated_at = ? WHERE id = ?")
            .bind(if archive { Some(t) } else { None })
            .bind(t)
            .bind(&id)
            .execute(&mut *tx)
            .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id),
            if archive { "project.archived" } else { "project.restored" }, Some(&id), json!({})).await?;
    }
    tx.commit().await?;
    get(State(state), auth, Path(id)).await
}

pub async fn archive(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<ProjectView>> {
    set_archived(state, auth, id, true).await
}

pub async fn restore(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<ProjectView>> {
    set_archived(state, auth, id, false).await
}
