//! A workspace's own AI keys, and its AI usage.

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
    error::{AppError, AppResult},
    security::{Role, now},
    state::AppState,
    workspace_ai::{self, PROVIDERS},
};

fn provider_name(id: &str) -> AppResult<&'static str> {
    PROVIDERS
        .iter()
        .find(|(p, _)| *p == id)
        .map(|(_, name)| *name)
        .ok_or_else(|| AppError::not_found("Peter supports the providers openrouter and gemini."))
}

#[derive(sqlx::FromRow)]
struct KeyRow {
    provider: String,
    key_hint: String,
    model: String,
    set_by_name: String,
    set_at: i64,
}

/// What keys the workspace drafts with. Never the keys themselves.
pub async fn get(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let rows: Vec<KeyRow> = sqlx::query_as(
        "SELECT k.provider, k.key_hint, k.model, u.name AS set_by_name, k.set_at
         FROM workspace_ai_keys k JOIN users u ON u.id = k.set_by WHERE k.workspace_id = ?",
    )
    .bind(&auth.workspace_id)
    .fetch_all(&state.db)
    .await?;
    let effective = workspace_ai::effective(&state, &auth.workspace_id).await.map_err(|_| AppError::internal())?;
    let server = &state.config.ai;
    let providers: Vec<Value> = PROVIDERS
        .iter()
        .map(|&(id, name)| {
            let own = rows.iter().find(|r| r.provider == id);
            let (server_key, server_model) = match id {
                "openrouter" => (server.openrouter_key.is_some(), &server.openrouter_model),
                _ => (server.gemini_key.is_some(), &server.gemini_model),
            };
            json!({
                "id": id,
                "name": name,
                "workspace_key": own.map(|r| json!({
                    "hint": format!("…{}", r.key_hint), "model": r.model, "set_by_name": r.set_by_name, "set_at": r.set_at,
                    "readable": !effective.unreadable.contains(&id),
                })),
                "server_key": server_key && server.shared,
                "server_model": server_model,
                "in_use": effective.source(id),
            })
        })
        .collect();
    let used = workspace_ai::shared_tokens_this_month(&state, &auth.workspace_id).await?;
    Ok(Json(json!({
        "providers": providers,
        "shared": server.shared,
        "shared_monthly_tokens": server.shared_monthly_tokens,
        "shared_tokens_this_month": used,
    })))
}

#[derive(Deserialize)]
pub struct KeyBody {
    key: Option<String>,
    model: Option<String>,
}

fn clean_model(model: &str) -> AppResult<String> {
    let m = model.trim();
    if m.chars().count() > 100 || !m.chars().all(|c| c.is_ascii_alphanumeric() || "._:/-".contains(c)) {
        return Err(AppError::bad_request("invalid_model", "A model name uses letters, digits and . _ : / - only, such as google/gemini-2.5-flash."));
    }
    Ok(m.to_string())
}

pub async fn set_key(
    State(state): State<AppState>,
    auth: Auth,
    Path(provider): Path<String>,
    Body(body): Body<KeyBody>,
) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let name = provider_name(&provider)?;
    let model = body.model.as_deref().map(clean_model).transpose()?;
    let key = body.key.as_deref().map(str::trim).filter(|k| !k.is_empty());
    if let Some(k) = key
        && (k.len() < 10 || k.len() > 300 || k.chars().any(|c| c.is_whitespace() || c.is_control()))
    {
        return Err(AppError::bad_request("invalid_key", format!("That does not look like a {name} key.")));
    }
    let t = now();
    let mut tx = state.db.begin().await?;
    let exists: Option<(String,)> = sqlx::query_as("SELECT key_hint FROM workspace_ai_keys WHERE workspace_id = ? AND provider = ?")
        .bind(&auth.workspace_id)
        .bind(&provider)
        .fetch_optional(&mut *tx)
        .await?;
    match key {
        Some(k) => {
            let encrypted = state.secret.encrypt(k).map_err(|_| AppError::internal())?;
            let hint: String = k.chars().rev().take(4).collect::<Vec<_>>().into_iter().rev().collect();
            sqlx::query(
                "INSERT INTO workspace_ai_keys (workspace_id, provider, key_encrypted, key_hint, model, set_by, set_at)
                 VALUES (?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (workspace_id, provider) DO UPDATE SET key_encrypted = excluded.key_encrypted,
                 key_hint = excluded.key_hint, model = CASE WHEN ? THEN excluded.model ELSE workspace_ai_keys.model END,
                 set_by = excluded.set_by, set_at = excluded.set_at",
            )
            .bind(&auth.workspace_id)
            .bind(&provider)
            .bind(encrypted)
            .bind(&hint)
            .bind(model.clone().unwrap_or_default())
            .bind(&auth.user_id)
            .bind(t)
            .bind(model.is_some())
            .execute(&mut *tx)
            .await?;
            audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "ai.key_set", None,
                json!({ "provider": provider, "hint": format!("…{hint}"), "model": model, "replaced": exists.is_some() })).await?;
        }
        None => {
            if exists.is_none() {
                return Err(AppError::bad_request("no_key", format!("Enter the workspace's {name} key.")));
            }
            let Some(m) = &model else {
                return Err(AppError::bad_request("nothing", "Send a new key or a model."));
            };
            sqlx::query("UPDATE workspace_ai_keys SET model = ?, set_by = ?, set_at = ? WHERE workspace_id = ? AND provider = ?")
                .bind(m)
                .bind(&auth.user_id)
                .bind(t)
                .bind(&auth.workspace_id)
                .bind(&provider)
                .execute(&mut *tx)
                .await?;
            audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "ai.model_set", None, json!({ "provider": provider, "model": m })).await?;
        }
    }
    tx.commit().await?;
    Ok(Json(json!({ "ok": true })))
}

pub async fn remove_key(State(state): State<AppState>, auth: Auth, Path(provider): Path<String>) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    provider_name(&provider)?;
    let mut tx = state.db.begin().await?;
    let done = sqlx::query("DELETE FROM workspace_ai_keys WHERE workspace_id = ? AND provider = ?")
        .bind(&auth.workspace_id)
        .bind(&provider)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("This workspace has no key for that provider."));
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "ai.key_removed", None, json!({ "provider": provider })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

/// Tries the key in use for a provider without spending anything.
pub async fn check(State(state): State<AppState>, auth: Auth, Path(provider): Path<String>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    provider_name(&provider)?;
    let effective = workspace_ai::effective(&state, &auth.workspace_id).await.map_err(|_| AppError::internal())?;
    let Some(source) = effective.source(&provider) else {
        return Err(AppError::bad_request("no_key", "There is no key to check for that provider."));
    };
    let result = crate::ai::check_key(&effective.config, &provider).await;
    Ok(Json(match result {
        Ok(message) => json!({ "ok": true, "key_source": source, "message": message }),
        Err(message) => json!({ "ok": false, "key_source": source, "message": message }),
    }))
}

#[derive(Deserialize)]
pub struct UsageQuery {
    days: Option<i64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct Totals {
    provider: String,
    key_source: String,
    calls: i64,
    failed: i64,
    prompt_tokens: i64,
    completion_tokens: i64,
    cost_usd: Option<f64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct ByPerson {
    user_id: String,
    name: String,
    calls: i64,
    tokens: i64,
    cost_usd: Option<f64>,
}

#[derive(Serialize, sqlx::FromRow)]
struct Recent {
    created_at: i64,
    user_name: String,
    session_id: Option<String>,
    session_title: Option<String>,
    step_id: Option<String>,
    provider: String,
    model: String,
    key_source: String,
    prompt_tokens: Option<i64>,
    completion_tokens: Option<i64>,
    cost_usd: Option<f64>,
    ok: bool,
    error: Option<String>,
}

/// AI calls over the last `days` days (30 by default, at most 366).
pub async fn usage(State(state): State<AppState>, auth: Auth, Query(q): Query<UsageQuery>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    let days = q.days.unwrap_or(30).clamp(1, 366);
    let since = now() - days * 86_400;
    let totals: Vec<Totals> = sqlx::query_as(
        "SELECT provider, key_source, COUNT(*) AS calls, SUM(1 - ok) AS failed,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens, COALESCE(SUM(completion_tokens), 0) AS completion_tokens,
                SUM(cost_usd) AS cost_usd
         FROM ai_usage WHERE workspace_id = ? AND created_at >= ? GROUP BY provider, key_source ORDER BY provider, key_source",
    )
    .bind(&auth.workspace_id)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    let people: Vec<ByPerson> = sqlx::query_as(
        "SELECT a.user_id, u.name, COUNT(*) AS calls,
                COALESCE(SUM(COALESCE(a.prompt_tokens, 0) + COALESCE(a.completion_tokens, 0)), 0) AS tokens, SUM(a.cost_usd) AS cost_usd
         FROM ai_usage a JOIN users u ON u.id = a.user_id
         WHERE a.workspace_id = ? AND a.created_at >= ? GROUP BY a.user_id, u.name ORDER BY tokens DESC",
    )
    .bind(&auth.workspace_id)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    let recent: Vec<Recent> = sqlx::query_as(
        "SELECT a.created_at, u.name AS user_name, a.session_id, s.title AS session_title, a.step_id, a.provider, a.model,
                a.key_source, a.prompt_tokens, a.completion_tokens, a.cost_usd, a.ok, a.error
         FROM ai_usage a JOIN users u ON u.id = a.user_id LEFT JOIN validation_sessions s ON s.id = a.session_id
         WHERE a.workspace_id = ? AND a.created_at >= ? ORDER BY a.created_at DESC, a.rowid DESC LIMIT 50",
    )
    .bind(&auth.workspace_id)
    .bind(since)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "days": days, "totals": totals, "people": people, "recent": recent })))
}
