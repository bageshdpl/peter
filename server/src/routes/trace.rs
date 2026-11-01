//! The traceability matrix of a session, built from its documents.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use serde_json::{Value, json};

use crate::{
    auth::{Auth, audit},
    drafting,
    error::{AppError, AppResult},
    export,
    security::{Role, new_id, now},
    state::{AppState, SessionEvent},
    trace,
};

/// The steps whose documents feed the matrix.
const SOURCES: &[&str] = &["requirements", "specification", "configuration", "installation", "testing"];

struct Loaded {
    title: String,
    archived: bool,
    docs: BTreeMap<String, String>,
    built_from: Vec<(String, String, i64)>,
}

async fn load(state: &AppState, auth: &Auth, session_id: &str) -> AppResult<Loaded> {
    let row: Option<(String, Option<i64>)> =
        sqlx::query_as("SELECT title, archived_at FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(session_id)
            .bind(&auth.workspace_id)
            .fetch_optional(&state.db)
            .await?;
    let (title, archived) = row.ok_or_else(|| AppError::not_found("No such session."))?;
    let rows: Vec<(String, String, i64, String)> = sqlx::query_as(
        "SELECT d.step_id, d.title, v.version, v.body FROM documents d
         JOIN document_versions v ON v.document_id = d.id AND v.version = d.current_version
         WHERE d.session_id = ?",
    )
    .bind(session_id)
    .fetch_all(&state.db)
    .await?;
    let mut docs = BTreeMap::new();
    let mut built_from = Vec::new();
    for step in SOURCES {
        if let Some((step_id, doc_title, version, body)) = rows.iter().find(|r| r.0 == *step) {
            docs.insert(step_id.clone(), body.clone());
            built_from.push((step_id.clone(), doc_title.clone(), *version));
        }
    }
    Ok(Loaded { title, archived: archived.is_some(), docs, built_from })
}

pub async fn get(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Json<Value>> {
    let loaded = load(&state, &auth, &session_id).await?;
    let matrix = trace::build(&loaded.docs);
    let built_from: Vec<Value> =
        loaded.built_from.iter().map(|(step, title, v)| json!({ "step_id": step, "title": title, "version": v })).collect();
    Ok(Json(json!({ "matrix": matrix, "built_from": built_from })))
}

/// Writes the matrix as a new version of the Traceability step's document.
pub async fn write(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    let loaded = load(&state, &auth, &session_id).await?;
    if loaded.archived {
        return Err(AppError::conflict("archived", "This session is archived. Restore it first."));
    }
    if !loaded.docs.contains_key("requirements") {
        return Err(AppError::bad_request("no_requirements", "Draft the Requirements and risk step first."));
    }
    let matrix = trace::build(&loaded.docs);
    let body = trace::markdown(&matrix, &loaded.built_from, &export::utc(now()));

    // Check citations the same way as any other version.
    let evidence = drafting::gather(&state, &session_id, "traceability").await.map_err(|_| AppError::internal())?;
    let (cited, unknown, gaps) = drafting::check_citations(&body, &drafting::known_ids(&evidence));
    let sources = evidence.sources.iter().map(|(tag, r, _)| (tag.clone(), r.clone())).collect();
    let citations = drafting::citations_json(&sources, &cited, &unknown, gaps);

    let t = now();
    let mut tx = state.db.begin().await?;
    let existing: Option<(String, String, i64)> =
        sqlx::query_as("SELECT id, status, current_version FROM documents WHERE session_id = ? AND step_id = 'traceability'")
            .bind(&session_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (doc_id, version) = match existing {
        Some((_, status, _)) if status == "drafting" => {
            return Err(AppError::conflict("drafting", "An AI draft of this document is being written. Wait for it to finish."));
        }
        Some((id, _, current)) => (id, current + 1),
        None => {
            let id = new_id();
            sqlx::query(
                "INSERT INTO documents (id, workspace_id, session_id, step_id, title, status, created_at, updated_at)
                 VALUES (?, ?, ?, 'traceability', 'Traceability matrix', 'ready', ?, ?)",
            )
            .bind(&id)
            .bind(&auth.workspace_id)
            .bind(&session_id)
            .bind(t)
            .bind(t)
            .execute(&mut *tx)
            .await?;
            (id, 1)
        }
    };
    sqlx::query(
        "INSERT INTO document_versions (id, document_id, version, body, origin, model, citations, created_by, created_at)
         VALUES (?, ?, ?, ?, 'generated', 'peter-trace', ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&doc_id)
    .bind(version)
    .bind(&body)
    .bind(citations.to_string())
    .bind(&auth.user_id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE documents SET status = 'ready', error = NULL, current_version = ?, updated_at = ? WHERE id = ?")
        .bind(version)
        .bind(t)
        .bind(&doc_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO step_states (session_id, step_id, status, updated_by, updated_at) VALUES (?, 'traceability', 'in_progress', ?, ?)
         ON CONFLICT (session_id, step_id) DO UPDATE SET status = 'in_progress', updated_by = excluded.updated_by,
         updated_at = excluded.updated_at WHERE step_states.status = 'not_started'",
    )
    .bind(&session_id)
    .bind(&auth.user_id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    let from: Vec<Value> = loaded.built_from.iter().map(|(s, _, v)| json!({ "step_id": s, "version": v })).collect();
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.generated", Some(&doc_id),
        json!({ "session_id": session_id, "step_id": "traceability", "version": version, "built_from": from,
                "requirements": matrix.summary.requirements, "open": matrix.summary.requirements - matrix.summary.passed })).await?;
    tx.commit().await?;

    let _ = state.events.send(SessionEvent {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.clone(),
        name: "document",
        data: json!({ "step_id": "traceability", "status": "ready" }),
    });
    Ok((StatusCode::CREATED, Json(json!({ "version": version }))))
}

/// The matrix, test list and open items as an Excel workbook.
pub async fn export_xlsx(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Response> {
    let loaded = load(&state, &auth, &session_id).await?;
    let m = trace::build(&loaded.docs);
    if m.rows.is_empty() {
        return Err(AppError::new(StatusCode::UNPROCESSABLE_ENTITY, "no_requirements", "There are no requirements to trace yet."));
    }
    use export::xlsx::Sheet;
    let matrix = Sheet {
        name: "Traceability".into(),
        head: ["Requirement", "Text", "Risk", "GxP critical", "Specification", "Tests", "Result", "Evidence"].map(String::from).to_vec(),
        rows: m
            .rows
            .iter()
            .map(|r| {
                vec![
                    r.requirement.id.clone(),
                    r.requirement.text.clone(),
                    r.requirement.risk.clone(),
                    r.requirement.gxp_critical.clone(),
                    r.specs.join(", "),
                    r.tests.join(", "),
                    trace::status_label(r.status).to_string(),
                    r.requirement.evidence.join(", "),
                ]
            })
            .collect(),
    };
    let tests = Sheet {
        name: "Tests".into(),
        head: ["Test", "Title", "Document", "Requirements", "Result", "Steps", "Passed", "Failed", "Evidence"].map(String::from).to_vec(),
        rows: m
            .tests
            .iter()
            .map(|t| {
                let outcome = match t.outcome {
                    trace::Outcome::Passed => "Passed",
                    trace::Outcome::Failed => "Failed",
                    trace::Outcome::NotExecuted => "Not yet executed",
                };
                vec![
                    t.id.clone(),
                    t.title.clone(),
                    t.step.clone(),
                    t.requirements.join(", "),
                    outcome.into(),
                    t.steps_total.to_string(),
                    t.steps_passed.to_string(),
                    t.steps_failed.to_string(),
                    t.evidence.join(", "),
                ]
            })
            .collect(),
    };
    let mut open: Vec<Vec<String>> = m
        .rows
        .iter()
        .filter(|r| r.status != trace::Status::Passed)
        .map(|r| vec![r.requirement.id.clone(), trace::status_label(r.status).into(), if r.high_risk { "High".into() } else { r.requirement.risk.clone() }])
        .collect();
    open.extend(m.warnings.iter().map(|w| vec!["—".into(), w.clone(), String::new()]));
    let gaps = Sheet { name: "Open items".into(), head: ["Item", "Issue", "Risk"].map(String::from).to_vec(), rows: open };

    let bytes = export::xlsx::write(&[matrix, tests, gaps], "Traceability matrix").map_err(|_| AppError::internal())?;
    let mut tx = state.db.begin().await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.exported", Some(&session_id),
        json!({ "step_id": "traceability", "format": "xlsx", "source": "matrix" })).await?;
    tx.commit().await?;
    super::review::attachment(
        bytes,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &export::file_name(&format!("{} traceability", loaded.title), "xlsx"),
    )
}
