//! The company's writing style: sample documents, and the style guide every
//! draft follows (learnt from the samples by AI, or written by a person).

use axum::{
    Json,
    extract::{Multipart, Path, State},
    http::StatusCode,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::Body;
use crate::{
    ai::{self, Prompt},
    auth::{Auth, audit},
    error::{AppError, AppResult},
    extract, prompts,
    security::{Role, new_id, now},
    state::AppState,
    workspace_ai::{self, CallFor},
};

const MAX_SAMPLES: i64 = 10;
const MAX_SAMPLE_BYTES: usize = 10 * 1024 * 1024;
/// How much of each sample, and of all of them, goes to the AI.
const SAMPLE_CHARS: usize = 12_000;
const ALL_SAMPLE_CHARS: usize = 40_000;
const MAX_GUIDE_CHARS: usize = 6_000;

#[derive(Serialize, sqlx::FromRow)]
struct SampleView {
    id: String,
    filename: String,
    sha256: String,
    chars: i64,
    created_by_name: String,
    created_at: i64,
}

#[derive(Serialize, sqlx::FromRow)]
struct GuideView {
    guide: String,
    source: String,
    model: Option<String>,
    updated_by_name: String,
    updated_at: i64,
}

async fn samples(state: &AppState, workspace_id: &str) -> AppResult<Vec<SampleView>> {
    Ok(sqlx::query_as(
        "SELECT s.id, s.filename, s.sha256, LENGTH(s.text) AS chars, u.name AS created_by_name, s.created_at
         FROM style_samples s JOIN users u ON u.id = s.created_by
         WHERE s.workspace_id = ? AND s.archived_at IS NULL ORDER BY s.created_at, s.id",
    )
    .bind(workspace_id)
    .fetch_all(&state.db)
    .await?)
}

pub async fn get(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let guide: Option<GuideView> = sqlx::query_as(
        "SELECT w.guide, w.source, w.model, u.name AS updated_by_name, w.updated_at
         FROM workspace_style w JOIN users u ON u.id = w.updated_by WHERE w.workspace_id = ?",
    )
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    Ok(Json(json!({ "guide": guide, "samples": samples(&state, &auth.workspace_id).await?, "max_samples": MAX_SAMPLES })))
}

pub async fn add_sample(State(state): State<AppState>, auth: Auth, multipart: Multipart) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Admin)?;
    let (filename, bytes, _) = super::library::read_upload(multipart, MAX_SAMPLE_BYTES).await?;
    let ext = filename.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let content_type = match ext.as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        _ => return Err(AppError::bad_request("file_type", "Upload a past document as Word (.docx), PDF, text or Markdown.")),
    };
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM style_samples WHERE workspace_id = ? AND archived_at IS NULL")
        .bind(&auth.workspace_id)
        .fetch_one(&state.db)
        .await?;
    if count >= MAX_SAMPLES {
        return Err(AppError::conflict("too_many", format!("Keep at most {MAX_SAMPLES} samples. Remove one first.")));
    }
    let text = extract::text_of(bytes.clone(), content_type.to_string())
        .await
        .map_err(|reason| AppError::bad_request("unreadable", format!("Peter could not read {filename}: {reason}.")))?;
    let text = extract::tidy(&text);
    if text.chars().count() < 200 {
        return Err(AppError::bad_request("unreadable", "That file has too little text to learn a style from."));
    }
    let sha = hex::encode(Sha256::digest(&bytes));
    let name: String = filename.chars().filter(|c| !c.is_control()).take(200).collect();
    let (id, t) = (new_id(), now());
    let mut tx = state.db.begin().await?;
    sqlx::query("INSERT INTO style_samples (id, workspace_id, filename, sha256, text, created_by, created_at) VALUES (?, ?, ?, ?, ?, ?, ?)")
        .bind(&id)
        .bind(&auth.workspace_id)
        .bind(&name)
        .bind(&sha)
        .bind(&text)
        .bind(&auth.user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "style.sample_added", Some(&id),
        json!({ "filename": name, "sha256": sha, "chars": text.chars().count() })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "chars": text.chars().count() }))))
}

pub async fn remove_sample(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let done = sqlx::query("UPDATE style_samples SET archived_at = ? WHERE id = ? AND workspace_id = ? AND archived_at IS NULL")
        .bind(now())
        .bind(&id)
        .bind(&auth.workspace_id)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("No such sample."));
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "style.sample_removed", Some(&id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

async fn save_guide(state: &AppState, auth: &Auth, guide: &str, source: &str, model: Option<&str>) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    if guide.trim().is_empty() {
        sqlx::query("DELETE FROM workspace_style WHERE workspace_id = ?").bind(&auth.workspace_id).execute(&mut *tx).await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "style.cleared", None, json!({})).await?;
    } else {
        sqlx::query(
            "INSERT INTO workspace_style (workspace_id, guide, source, model, updated_by, updated_at) VALUES (?, ?, ?, ?, ?, ?)
             ON CONFLICT (workspace_id) DO UPDATE SET guide = excluded.guide, source = excluded.source, model = excluded.model,
             updated_by = excluded.updated_by, updated_at = excluded.updated_at",
        )
        .bind(&auth.workspace_id)
        .bind(guide)
        .bind(source)
        .bind(model)
        .bind(&auth.user_id)
        .bind(now())
        .execute(&mut *tx)
        .await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), if source == "ai" { "style.learnt" } else { "style.edited" }, None,
            json!({ "source": source, "model": model, "chars": guide.chars().count() })).await?;
    }
    tx.commit().await?;
    Ok(())
}

#[derive(Deserialize)]
pub struct GuideBody {
    guide: String,
}

/// Writes or edits the guide by hand. An empty guide stops drafts using one.
pub async fn set_guide(State(state): State<AppState>, auth: Auth, Body(body): Body<GuideBody>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let guide = body.guide.trim();
    if guide.chars().count() > MAX_GUIDE_CHARS {
        return Err(AppError::bad_request("too_long", format!("Keep the style guide under {MAX_GUIDE_CHARS} characters.")));
    }
    save_guide(&state, &auth, guide, "person", None).await?;
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
pub struct LearnBody {
    provider: String,
}

/// Asks the AI to describe the samples' house style, and keeps the result.
pub async fn learn(State(state): State<AppState>, auth: Auth, Body(body): Body<LearnBody>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let effective = workspace_ai::effective(&state, &auth.workspace_id).await.map_err(|_| AppError::internal())?;
    let Some(source) = effective.source(&body.provider) else {
        return Err(AppError::bad_request("unknown_provider", "Choose an AI provider that has a key for this workspace."));
    };
    if source == "server"
        && let Some(limit) = state.config.ai.shared_monthly_tokens
        && workspace_ai::shared_tokens_this_month(&state, &auth.workspace_id).await? >= limit
    {
        return Err(AppError::new(StatusCode::TOO_MANY_REQUESTS, "ai_limit", format!(
            "This workspace has used its {limit} tokens of shared AI this month. An admin can add the workspace's own key."
        )));
    }
    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT filename, text FROM style_samples WHERE workspace_id = ? AND archived_at IS NULL ORDER BY created_at, id",
    )
    .bind(&auth.workspace_id)
    .fetch_all(&state.db)
    .await?;
    if rows.is_empty() {
        return Err(AppError::bad_request("no_samples", "Add at least one past document first."));
    }
    let mut budget = ALL_SAMPLE_CHARS;
    let samples: Vec<(String, String)> = rows
        .into_iter()
        .map(|(name, text)| {
            let take = SAMPLE_CHARS.min(budget);
            let cut: String = text.chars().take(take).collect();
            budget = budget.saturating_sub(cut.chars().count());
            (name, cut)
        })
        .filter(|(_, t)| !t.is_empty())
        .collect();
    let user = prompts::style_user_prompt(&samples);
    let result = ai::complete(&effective.config, &body.provider, Prompt { system: prompts::STYLE_SYSTEM, user: &user }).await;
    workspace_ai::record(&state, CallFor { workspace_id: &auth.workspace_id, user_id: &auth.user_id, session_id: None, step_id: None,
        provider: &body.provider }, source, &result).await;
    let completion = result.map_err(|e| AppError::new(StatusCode::BAD_GATEWAY, "ai_failed", e))?;
    let guide: String = completion.text.trim().trim_start_matches("```markdown").trim_start_matches("```").trim_end_matches("```").trim()
        .chars().take(MAX_GUIDE_CHARS).collect();
    if guide.is_empty() {
        return Err(AppError::new(StatusCode::BAD_GATEWAY, "ai_failed", "The AI returned no style guide. Try again."));
    }
    save_guide(&state, &auth, &guide, "ai", Some(&completion.model)).await?;
    Ok(Json(json!({ "guide": guide, "model": completion.model })))
}
