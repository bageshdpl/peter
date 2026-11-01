//! Guided steps (M3) and the documents drafted in them (M4).

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::Body;
use crate::{
    auth::{Auth, audit},
    catalog,
    drafting::{self, DraftJob},
    error::{AppError, AppResult},
    prompts,
    security::{Role, new_id, now},
    state::{AppState, SessionEvent},
};

const STATUSES: &[&str] = &["not_started", "in_progress", "reviewed", "skipped"];

/// The session's validation type, checking the session is in the caller's
/// workspace and, for changes, not archived.
async fn session_type(state: &AppState, auth: &Auth, session_id: &str, for_change: bool) -> AppResult<&'static catalog::ValidationType> {
    let row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT validation_type, archived_at FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(session_id)
            .bind(&auth.workspace_id)
            .fetch_optional(&state.db)
            .await?;
    let (kind, archived) = row.ok_or_else(|| AppError::not_found("No such session."))?;
    if for_change && archived.is_some() {
        return Err(AppError::conflict("archived", "This session is archived. Restore it first."));
    }
    catalog::find(&kind).ok_or_else(AppError::internal)
}

fn check_step(kind: &catalog::ValidationType, step_id: &str) -> AppResult<()> {
    if kind.steps.iter().any(|s| s.id == step_id) {
        Ok(())
    } else {
        Err(AppError::not_found("This session has no such step."))
    }
}

fn publish(state: &AppState, auth: &Auth, session_id: &str, name: &'static str, data: Value) {
    let _ = state.events.send(SessionEvent {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.to_string(),
        name,
        data,
    });
}

// ---------- M3: steps ----------

#[derive(Deserialize)]
pub struct StatusBody {
    status: String,
}

pub async fn set_step_status(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Body(body): Body<StatusBody>,
) -> AppResult<StatusCode> {
    auth.require(Role::Member)?;
    let kind = session_type(&state, &auth, &session_id, true).await?;
    check_step(kind, &step_id)?;
    if !STATUSES.contains(&body.status.as_str()) {
        return Err(AppError::bad_request("invalid_status", format!("Status must be one of: {}.", STATUSES.join(", "))));
    }
    let mut tx = state.db.begin().await?;
    let previous: Option<(String,)> = sqlx::query_as("SELECT status FROM step_states WHERE session_id = ? AND step_id = ?")
        .bind(&session_id)
        .bind(&step_id)
        .fetch_optional(&mut *tx)
        .await?;
    let from = previous.map(|(s,)| s).unwrap_or_else(|| "not_started".into());
    if from != body.status {
        sqlx::query(
            "INSERT INTO step_states (session_id, step_id, status, updated_by, updated_at) VALUES (?, ?, ?, ?, ?)
             ON CONFLICT (session_id, step_id) DO UPDATE SET status = excluded.status, updated_by = excluded.updated_by,
             updated_at = excluded.updated_at",
        )
        .bind(&session_id)
        .bind(&step_id)
        .bind(&body.status)
        .bind(&auth.user_id)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "step.status_changed", Some(&session_id),
            json!({ "step_id": step_id, "from": from, "to": body.status })).await?;
    }
    tx.commit().await?;
    publish(&state, &auth, &session_id, "step", json!({ "step_id": step_id, "status": body.status }));
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
pub struct CurrentBody {
    step_id: String,
}

/// Moves the session to a step. Opening a step that was not started marks it in progress.
pub async fn set_current(
    State(state): State<AppState>,
    auth: Auth,
    Path(session_id): Path<String>,
    Body(body): Body<CurrentBody>,
) -> AppResult<StatusCode> {
    auth.require(Role::Member)?;
    let kind = session_type(&state, &auth, &session_id, true).await?;
    check_step(kind, &body.step_id)?;
    let t = now();
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE validation_sessions SET current_step = ?, updated_at = ? WHERE id = ?")
        .bind(&body.step_id)
        .bind(t)
        .bind(&session_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO step_states (session_id, step_id, status, updated_by, updated_at) VALUES (?, ?, 'in_progress', ?, ?)
         ON CONFLICT (session_id, step_id) DO UPDATE SET status = 'in_progress', updated_by = excluded.updated_by,
         updated_at = excluded.updated_at WHERE step_states.status = 'not_started'",
    )
    .bind(&session_id)
    .bind(&body.step_id)
    .bind(&auth.user_id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "step", json!({ "current_step": body.step_id }));
    Ok(StatusCode::NO_CONTENT)
}

// ---------- M4: documents ----------

pub async fn providers(State(state): State<AppState>, _auth: Auth) -> Json<Value> {
    Json(json!({ "providers": state.config.ai.providers() }))
}

#[derive(Serialize, sqlx::FromRow)]
struct VersionSummary {
    version: i64,
    origin: String,
    provider: Option<String>,
    model: Option<String>,
    created_by_name: String,
    created_at: i64,
}

pub async fn get(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let kind = session_type(&state, &auth, &session_id, false).await?;
    check_step(kind, &step_id)?;
    let spec = prompts::spec(&step_id);
    let doc: Option<(String, String, String, Option<String>, i64, i64)> = sqlx::query_as(
        "SELECT id, title, status, error, current_version, updated_at FROM documents WHERE session_id = ? AND step_id = ?",
    )
    .bind(&session_id)
    .bind(&step_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((id, title, status, error, current_version, updated_at)) = doc else {
        return Ok(Json(json!({
            "document": null,
            "title": spec.as_ref().map(|s| s.title),
            "draftable": spec.is_some(),
        })));
    };
    let versions: Vec<VersionSummary> = sqlx::query_as(
        "SELECT v.version, v.origin, v.provider, v.model, u.name AS created_by_name, v.created_at
         FROM document_versions v JOIN users u ON u.id = v.created_by WHERE v.document_id = ? ORDER BY v.version DESC",
    )
    .bind(&id)
    .fetch_all(&state.db)
    .await?;
    let current = version_body(&state, &session_id, &id, current_version).await?;
    Ok(Json(json!({
        "document": {
            "id": id, "step_id": step_id, "title": title, "status": status, "error": error,
            "current_version": current_version, "updated_at": updated_at,
        },
        "current": current,
        "versions": versions,
        "title": title,
        "draftable": spec.is_some(),
    })))
}

#[derive(sqlx::FromRow)]
struct VersionRow {
    body: String,
    origin: String,
    provider: Option<String>,
    model: Option<String>,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    citations: String,
    created_by_name: String,
    created_at: i64,
}

/// One version's text, with what its citations point at so the page can link them.
async fn version_body(state: &AppState, session_id: &str, document_id: &str, version: i64) -> AppResult<Value> {
    if version == 0 {
        return Ok(Value::Null);
    }
    let row: Option<VersionRow> = sqlx::query_as(
        "SELECT v.body, v.origin, v.provider, v.model, v.prompt_tokens, v.completion_tokens, v.citations,
                u.name AS created_by_name, v.created_at
         FROM document_versions v JOIN users u ON u.id = v.created_by WHERE v.document_id = ? AND v.version = ?",
    )
    .bind(document_id)
    .bind(version)
    .fetch_optional(&state.db)
    .await?;
    let VersionRow { body, origin, provider, model, prompt_tokens, completion_tokens, citations, created_by_name: by, created_at: at } =
        row.ok_or_else(|| AppError::not_found("No such version."))?;
    let citations: Value = serde_json::from_str(&citations).unwrap_or_else(|_| json!({}));

    // Observations cited by number: what they were, so the page can show them.
    let seqs: Vec<i64> = citations["cited"]
        .as_array()
        .map(|a| a.iter().filter_map(|c| c.as_str()?.strip_prefix('O')?.parse().ok()).collect())
        .unwrap_or_default();
    let mut observations = serde_json::Map::new();
    if !seqs.is_empty() {
        let rows: Vec<(String, i64, String, String, String, bool)> = sqlx::query_as(
            "SELECT id, seq, kind, target_label, page_title, (screenshot_type IS NOT NULL)
             FROM observations WHERE session_id = ? AND deleted_at IS NULL",
        )
        .bind(session_id)
        .fetch_all(&state.db)
        .await?;
        for (id, seq, kind, label, page, shot) in rows.into_iter().filter(|r| seqs.contains(&r.1)) {
            observations.insert(format!("O{seq}"), json!({ "id": id, "seq": seq, "kind": kind,
                "label": if label.is_empty() { page } else { label }, "has_screenshot": shot }));
        }
    }
    Ok(json!({
        "version": version, "body": body, "origin": origin, "provider": provider, "model": model,
        "prompt_tokens": prompt_tokens, "completion_tokens": completion_tokens,
        "citations": citations, "observations": observations, "created_by_name": by, "created_at": at,
    }))
}

pub async fn get_version(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id, version)): Path<(String, String, i64)>,
) -> AppResult<Json<Value>> {
    session_type(&state, &auth, &session_id, false).await?;
    let doc: Option<(String,)> = sqlx::query_as("SELECT id FROM documents WHERE session_id = ? AND step_id = ?")
        .bind(&session_id)
        .bind(&step_id)
        .fetch_optional(&state.db)
        .await?;
    let (id,) = doc.ok_or_else(|| AppError::not_found("No document for this step yet."))?;
    Ok(Json(version_body(&state, &session_id, &id, version).await?))
}

#[derive(Deserialize)]
pub struct DraftBody {
    provider: String,
    #[serde(default)]
    instructions: String,
}

pub async fn start_draft(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Body(body): Body<DraftBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    let kind = session_type(&state, &auth, &session_id, true).await?;
    check_step(kind, &step_id)?;
    let spec = prompts::spec(&step_id).ok_or_else(|| AppError::bad_request("not_draftable", "This step has no document to draft."))?;
    if !state.config.ai.providers().iter().any(|p| p.id == body.provider) {
        return Err(AppError::bad_request("unknown_provider", "That AI provider is not set up on this server."));
    }
    if body.instructions.chars().count() > 2000 {
        return Err(AppError::bad_request("invalid_field", "Keep extra instructions under 2000 characters."));
    }

    let t = now();
    let mut tx = state.db.begin().await?;
    let existing: Option<(String, String)> = sqlx::query_as("SELECT id, status FROM documents WHERE session_id = ? AND step_id = ?")
        .bind(&session_id)
        .bind(&step_id)
        .fetch_optional(&mut *tx)
        .await?;
    let document_id = match existing {
        Some((_, status)) if status == "drafting" => {
            return Err(AppError::conflict("drafting", "A draft is already being written for this step."));
        }
        Some((id, _)) => {
            sqlx::query("UPDATE documents SET status = 'drafting', error = NULL, updated_at = ? WHERE id = ?")
                .bind(t)
                .bind(&id)
                .execute(&mut *tx)
                .await?;
            id
        }
        None => {
            let id = new_id();
            sqlx::query(
                "INSERT INTO documents (id, workspace_id, session_id, step_id, title, status, created_at, updated_at)
                 VALUES (?, ?, ?, ?, ?, 'drafting', ?, ?)",
            )
            .bind(&id)
            .bind(&auth.workspace_id)
            .bind(&session_id)
            .bind(&step_id)
            .bind(spec.title)
            .bind(t)
            .bind(t)
            .execute(&mut *tx)
            .await?;
            id
        }
    };
    tx.commit().await?;

    publish(&state, &auth, &session_id, "document", json!({ "step_id": step_id, "status": "drafting" }));
    let job = DraftJob {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.clone(),
        step_id: step_id.clone(),
        document_id: document_id.clone(),
        user_id: auth.user_id.clone(),
        provider: body.provider,
        extra: body.instructions,
    };
    tokio::spawn(drafting::run(state.clone(), job));
    Ok((StatusCode::ACCEPTED, Json(json!({ "document_id": document_id, "status": "drafting" }))))
}

#[derive(Deserialize)]
pub struct SaveBody {
    body: String,
    /// The version the edit started from; refuses the save if someone saved since.
    base_version: i64,
}

pub async fn save(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Body(body): Body<SaveBody>,
) -> AppResult<Json<Value>> {
    auth.require(Role::Member)?;
    let kind = session_type(&state, &auth, &session_id, true).await?;
    check_step(kind, &step_id)?;
    if body.body.trim().is_empty() || body.body.chars().count() > 200_000 {
        return Err(AppError::bad_request("invalid_field", "The document must have text, and at most 200,000 characters."));
    }
    // Read the evidence before the transaction holds a connection.
    let evidence = drafting::gather(&state, &session_id, &step_id).await.map_err(|_| AppError::internal())?;
    let known = drafting::known_ids(&evidence);
    let (cited, unknown, gaps) = drafting::check_citations(&body.body, &known);

    let mut tx = state.db.begin().await?;
    let doc: Option<(String, String, i64)> =
        sqlx::query_as("SELECT id, status, current_version FROM documents WHERE session_id = ? AND step_id = ?")
            .bind(&session_id)
            .bind(&step_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (id, status, current) = doc.ok_or_else(|| AppError::not_found("Draft this document first."))?;
    if status == "drafting" {
        return Err(AppError::conflict("drafting", "Wait for the draft being written to finish."));
    }
    if body.base_version != current {
        return Err(AppError::conflict("stale", "Someone saved a newer version while you were editing. Reload to see it."));
    }
    // Keep what the source numbers meant in the version this edit started from.
    let base: Option<(String,)> = sqlx::query_as("SELECT citations FROM document_versions WHERE document_id = ? AND version = ?")
        .bind(&id)
        .bind(current)
        .fetch_optional(&mut *tx)
        .await?;
    let base: Value = base.and_then(|(c,)| serde_json::from_str(&c).ok()).unwrap_or_else(|| json!({}));
    let sources = serde_json::from_value(base["sources"].clone()).unwrap_or_default();
    let citations = drafting::citations_json(&sources, &cited, &unknown, gaps);

    let version = current + 1;
    sqlx::query(
        "INSERT INTO document_versions (id, document_id, version, body, origin, citations, created_by, created_at)
         VALUES (?, ?, ?, ?, 'person', ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&id)
    .bind(version)
    .bind(&body.body)
    .bind(citations.to_string())
    .bind(&auth.user_id)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE documents SET status = 'ready', error = NULL, current_version = ?, updated_at = ? WHERE id = ?")
        .bind(version)
        .bind(now())
        .bind(&id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.edited", Some(&id),
        json!({ "session_id": session_id, "step_id": step_id, "version": version })).await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "document", json!({ "step_id": step_id, "status": "ready" }));
    get(State(state), auth, Path((session_id, step_id))).await
}
