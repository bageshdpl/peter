//! Steps the Observer extension records in the system under validation, and
//! the live stream that shows them on the session page.

use std::{convert::Infallible, time::Duration};

use axum::{
    Json,
    body::Body as HttpBody,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::{
        Response,
        sse::{Event, KeepAlive, Sse},
    },
};
use base64::{Engine as _, engine::general_purpose::STANDARD};
use futures_util::stream::{self, Stream};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::broadcast::error::RecvError;

use super::{Body, projects::clean_text};
use crate::{
    auth::{Auth, audit},
    error::{AppError, AppResult},
    security::{Role, new_id, now},
    state::{AppState, SessionEvent},
};

pub const MAX_SCREENSHOT_BYTES: usize = 8 * 1024 * 1024;
const KINDS: &[&str] = &["navigate", "click", "input", "select", "submit", "note", "capture"];

#[derive(Serialize, sqlx::FromRow)]
pub struct ObservationView {
    id: String,
    seq: i64,
    kind: String,
    url: String,
    page_title: String,
    target_label: String,
    target_selector: String,
    value: String,
    masked: bool,
    note: String,
    has_screenshot: bool,
    screenshot_type: Option<String>,
    captured_at_ms: i64,
    created_by_name: String,
    created_at: i64,
}

const OBSERVATION_SELECT: &str = "
    SELECT o.id, o.seq, o.kind, o.url, o.page_title, o.target_label, o.target_selector, o.value,
           o.masked, o.note, (o.screenshot_type IS NOT NULL) AS has_screenshot, o.screenshot_type,
           o.captured_at_ms, u.name AS created_by_name, o.created_at
    FROM observations o JOIN users u ON u.id = o.created_by";

/// Checks the session is in the caller's workspace. Returns whether it is archived.
async fn session_in_workspace(state: &AppState, auth: &Auth, session_id: &str) -> AppResult<bool> {
    let row: Option<(Option<i64>,)> =
        sqlx::query_as("SELECT archived_at FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(session_id)
            .bind(&auth.workspace_id)
            .fetch_optional(&state.db)
            .await?;
    row.map(|(a,)| a.is_some()).ok_or_else(|| AppError::not_found("No such session."))
}

fn publish(state: &AppState, auth: &Auth, session_id: &str, name: &'static str, data: Value) {
    // No open pages means no receivers; that is fine.
    let _ = state.events.send(SessionEvent {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.to_string(),
        name,
        data,
    });
}

#[derive(Deserialize)]
pub struct ListQuery {
    /// Only steps after this sequence number (to catch up after reconnecting).
    after_seq: Option<i64>,
}

pub async fn list(
    State(state): State<AppState>,
    auth: Auth,
    Path(session_id): Path<String>,
    Query(q): Query<ListQuery>,
) -> AppResult<Json<Value>> {
    session_in_workspace(&state, &auth, &session_id).await?;
    let sql = format!("{OBSERVATION_SELECT} WHERE o.session_id = ? AND o.deleted_at IS NULL AND o.seq > ? ORDER BY o.seq");
    let rows: Vec<ObservationView> = sqlx::query_as(sqlx::AssertSqlSafe(sql))
        .bind(&session_id)
        .bind(q.after_seq.unwrap_or(0))
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!({ "observations": rows })))
}

#[derive(Deserialize)]
pub struct CreateBody {
    kind: String,
    #[serde(default)]
    url: String,
    #[serde(default)]
    page_title: String,
    #[serde(default)]
    target_label: String,
    #[serde(default)]
    target_selector: String,
    #[serde(default)]
    value: String,
    #[serde(default)]
    masked: bool,
    #[serde(default)]
    note: String,
    captured_at_ms: Option<i64>,
    /// A `data:image/jpeg;base64,...` or `data:image/png;base64,...` URL.
    screenshot: Option<String>,
}

/// Decodes a screenshot data URL, checking that the bytes really are the image type claimed.
fn decode_screenshot(data_url: &str) -> AppResult<(&'static str, Vec<u8>)> {
    let bad = || AppError::bad_request("invalid_screenshot", "The screenshot must be a JPEG or PNG data URL.");
    let (kind, b64) = if let Some(rest) = data_url.strip_prefix("data:image/jpeg;base64,") {
        ("image/jpeg", rest)
    } else if let Some(rest) = data_url.strip_prefix("data:image/png;base64,") {
        ("image/png", rest)
    } else {
        return Err(bad());
    };
    if b64.len() > MAX_SCREENSHOT_BYTES / 3 * 4 + 4 {
        return Err(AppError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", "The screenshot is too large."));
    }
    let bytes = STANDARD.decode(b64).map_err(|_| bad())?;
    let magic_ok = match kind {
        "image/jpeg" => bytes.starts_with(&[0xFF, 0xD8, 0xFF]),
        _ => bytes.starts_with(b"\x89PNG\r\n\x1a\n"),
    };
    if !magic_ok {
        return Err(bad());
    }
    Ok((kind, bytes))
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(session_id): Path<String>,
    Body(body): Body<CreateBody>,
) -> AppResult<(StatusCode, Json<ObservationView>)> {
    auth.require(Role::Member)?;
    if session_in_workspace(&state, &auth, &session_id).await? {
        return Err(AppError::conflict("archived", "This session is archived. Restore it before recording."));
    }
    if !KINDS.contains(&body.kind.as_str()) {
        return Err(AppError::bad_request("invalid_kind", format!("Kind must be one of: {}.", KINDS.join(", "))));
    }
    let url = clean_text(&body.url, "URL", 2000, false)?;
    let page_title = clean_text(&body.page_title, "page title", 300, false)?;
    let target_label = clean_text(&body.target_label, "target label", 300, false)?;
    let target_selector = clean_text(&body.target_selector, "target selector", 500, false)?;
    // Values keep their spacing; only the length is limited.
    if body.value.chars().count() > 2000 {
        return Err(AppError::bad_request("invalid_field", "The value must be at most 2000 characters."));
    }
    let note = clean_text(&body.note, "note", 4000, false)?;
    if body.kind == "note" && note.is_empty() {
        return Err(AppError::bad_request("invalid_field", "A note needs some text."));
    }

    // The browser's clock is used only when it is plausible.
    let now_ms = now() * 1000;
    let captured_at_ms = body
        .captured_at_ms
        .filter(|t| (*t - now_ms).abs() < 24 * 3600 * 1000)
        .unwrap_or(now_ms);

    let id = new_id();
    let screenshot = match body.screenshot.as_deref() {
        Some(data_url) => {
            let (kind, bytes) = decode_screenshot(data_url)?;
            let sha = hex::encode(Sha256::digest(&bytes));
            let path = state.config.data_dir.join("screenshots").join(&id);
            tokio::fs::write(&path, &bytes).await.map_err(|e| {
                tracing::error!(error = %e, "could not store screenshot");
                AppError::internal()
            })?;
            Some((kind, bytes.len() as i64, sha))
        }
        None => None,
    };

    let inserted: AppResult<()> = async {
        // The sequence number is computed inside the INSERT, so two steps
        // arriving together cannot get the same number.
        sqlx::query(
            "INSERT INTO observations
             (id, workspace_id, session_id, seq, kind, url, page_title, target_label, target_selector, value, masked, note,
              screenshot_type, screenshot_bytes, screenshot_sha256, captured_at_ms, created_by, created_at)
             VALUES (?, ?, ?, (SELECT COALESCE(MAX(seq), 0) + 1 FROM observations WHERE session_id = ?),
                     ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&auth.workspace_id)
        .bind(&session_id)
        .bind(&session_id)
        .bind(&body.kind)
        .bind(&url)
        .bind(&page_title)
        .bind(&target_label)
        .bind(&target_selector)
        .bind(&body.value)
        .bind(body.masked)
        .bind(&note)
        .bind(screenshot.as_ref().map(|s| s.0))
        .bind(screenshot.as_ref().map(|s| s.1))
        .bind(screenshot.as_ref().map(|s| s.2.clone()))
        .bind(captured_at_ms)
        .bind(&auth.user_id)
        .bind(now())
        .execute(&state.db)
        .await?;
        sqlx::query("UPDATE validation_sessions SET updated_at = ? WHERE id = ?")
            .bind(now())
            .bind(&session_id)
            .execute(&state.db)
            .await?;
        Ok(())
    }
    .await;
    if let Err(err) = inserted {
        if screenshot.is_some() {
            let _ = tokio::fs::remove_file(state.config.data_dir.join("screenshots").join(&id)).await;
        }
        return Err(err);
    }

    let sql = format!("{OBSERVATION_SELECT} WHERE o.id = ?");
    let view: ObservationView = sqlx::query_as(sqlx::AssertSqlSafe(sql)).bind(&id).fetch_one(&state.db).await?;
    publish(&state, &auth, &session_id, "observation", serde_json::to_value(&view).unwrap_or(Value::Null));
    Ok((StatusCode::CREATED, Json(view)))
}

pub async fn screenshot(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, observation_id)): Path<(String, String)>,
) -> AppResult<Response> {
    let row: Option<(Option<String>,)> = sqlx::query_as(
        "SELECT screenshot_type FROM observations
         WHERE id = ? AND session_id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(&observation_id)
    .bind(&session_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    let content_type = row.and_then(|(t,)| t).ok_or_else(|| AppError::not_found("No screenshot for this step."))?;
    let bytes = tokio::fs::read(state.config.data_dir.join("screenshots").join(&observation_id))
        .await
        .map_err(|e| {
            tracing::error!(error = %e, %observation_id, "stored screenshot is missing");
            AppError::internal()
        })?;
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CACHE_CONTROL, "private, max-age=86400, immutable")
        .header("X-Content-Type-Options", "nosniff")
        .body(HttpBody::from(bytes))
        .map_err(|_| AppError::internal())
}

/// Removes a step from the session. It stays in the database as a record of
/// what was captured and who removed it.
pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, observation_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    auth.require(Role::Member)?;
    if session_in_workspace(&state, &auth, &session_id).await? {
        return Err(AppError::conflict("archived", "This session is archived."));
    }
    let mut tx = state.db.begin().await?;
    let row: Option<(i64, String)> = sqlx::query_as(
        "SELECT seq, kind FROM observations WHERE id = ? AND session_id = ? AND deleted_at IS NULL",
    )
    .bind(&observation_id)
    .bind(&session_id)
    .fetch_optional(&mut *tx)
    .await?;
    let (seq, kind) = row.ok_or_else(|| AppError::not_found("No such step."))?;
    sqlx::query("UPDATE observations SET deleted_at = ?, deleted_by = ? WHERE id = ?")
        .bind(now())
        .bind(&auth.user_id)
        .bind(&observation_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "observation.removed", Some(&observation_id),
        json!({ "session_id": session_id, "seq": seq, "kind": kind })).await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "observation_removed", json!({ "id": observation_id }));
    Ok(StatusCode::NO_CONTENT)
}

/// Server-Sent Events for one session: `observation` and `observation_removed`.
pub async fn events(
    State(state): State<AppState>,
    auth: Auth,
    Path(session_id): Path<String>,
) -> AppResult<Sse<impl Stream<Item = Result<Event, Infallible>>>> {
    session_in_workspace(&state, &auth, &session_id).await?;
    let rx = state.events.subscribe();
    let workspace_id = auth.workspace_id.clone();
    let hello = Event::default().event("ready").data("{}");

    let updates = stream::unfold(rx, move |mut rx| {
        let (workspace_id, session_id) = (workspace_id.clone(), session_id.clone());
        async move {
            loop {
                match rx.recv().await {
                    Ok(ev) if ev.workspace_id == workspace_id && ev.session_id == session_id => {
                        let event = Event::default().event(ev.name).data(ev.data.to_string());
                        return Some((Ok(event), rx));
                    }
                    Ok(_) => continue,
                    // Missed events: tell the page to reload the list.
                    Err(RecvError::Lagged(_)) => return Some((Ok(Event::default().event("resync").data("{}")), rx)),
                    Err(RecvError::Closed) => return None,
                }
            }
        }
    });
    let stream = futures_util::StreamExt::chain(stream::once(async { Ok(hello) }), updates);
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(20))))
}
