use axum::{Json, extract::State, http::StatusCode};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Body, clean_name};
use crate::{
    auth::{Auth, audit},
    error::AppResult,
    security::{Role, new_id, now},
    state::AppState,
};

pub async fn get(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let (id, name, created_at): (String, String, i64) =
        sqlx::query_as("SELECT id, name, created_at FROM workspaces WHERE id = ?")
            .bind(&auth.workspace_id)
            .fetch_one(&state.db)
            .await?;
    Ok(Json(json!({ "id": id, "name": name, "created_at": created_at, "role": auth.role })))
}

#[derive(Deserialize)]
pub struct NameBody {
    name: String,
}

pub async fn rename(State(state): State<AppState>, auth: Auth, Body(body): Body<NameBody>) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let name = clean_name(&body.name, "workspace name")?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE workspaces SET name = ? WHERE id = ?")
        .bind(&name)
        .bind(&auth.workspace_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "workspace.renamed", Some(&auth.workspace_id),
        json!({ "name": name })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Creates another workspace with the caller as its owner. Use
/// `/v1/auth/switch` to move into it.
pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Body(body): Body<NameBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let name = clean_name(&body.name, "workspace name")?;
    let (id, t) = (new_id(), now());
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO workspaces (id, name, created_at) VALUES (?, ?, ?)")
        .bind(&id)
        .bind(&name)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    sqlx::query("INSERT INTO memberships (workspace_id, user_id, role, created_at) VALUES (?, ?, 'owner', ?)")
        .bind(&id)
        .bind(&auth.user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&id), Some(&auth.user_id), "workspace.created", Some(&id), json!({ "name": name })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "name": name, "role": "owner" }))))
}
