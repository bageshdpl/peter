//! Controls: a project's SOP controls, and each session's coverage of them and
//! of the regulations in scope.

use std::collections::BTreeMap;

use axum::{
    Json,
    extract::{Path, State},
    http::StatusCode,
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use super::{Body, projects::project_in_workspace};
use crate::{
    auth::{Auth, audit},
    controls::{self, Control, Decision, Detect},
    error::{AppError, AppResult},
    export,
    security::{Role, new_id, now},
    state::{AppState, SessionEvent},
    trace,
};

const MAX_CONTROL_CHARS: usize = 1000;
const MAX_PROJECT_CONTROLS: i64 = 1000;

#[derive(Serialize, sqlx::FromRow)]
pub struct ProjectControlView {
    id: String,
    number: i64,
    text: String,
    section: String,
    source_id: Option<String>,
    source_filename: Option<String>,
    created_by_name: String,
    created_at: i64,
}

#[derive(Serialize)]
struct ProjectControlOut {
    #[serde(flatten)]
    row: ProjectControlView,
    reference: String,
}

async fn project_controls(state: &AppState, project_id: &str) -> AppResult<Vec<ProjectControlView>> {
    Ok(sqlx::query_as(
        "SELECT c.id, c.number, c.text, c.section, c.source_id, f.filename AS source_filename,
                u.name AS created_by_name, c.created_at
         FROM project_controls c JOIN users u ON u.id = c.created_by
         LEFT JOIN project_sources f ON f.id = c.source_id
         WHERE c.project_id = ? AND c.archived_at IS NULL ORDER BY c.number",
    )
    .bind(project_id)
    .fetch_all(&state.db)
    .await?)
}

fn out(rows: Vec<ProjectControlView>) -> Vec<ProjectControlOut> {
    rows.into_iter().map(|row| ProjectControlOut { reference: controls::reference(row.number), row }).collect()
}

/// Checks the project and that it can be changed.
async fn writable_project(state: &AppState, auth: &Auth, project_id: &str) -> AppResult<()> {
    auth.require(Role::Member)?;
    let mut conn = state.db.acquire().await?;
    if project_in_workspace(&mut conn, &auth.workspace_id, project_id).await? {
        return Err(AppError::conflict("archived", "Restore this project before changing its controls."));
    }
    Ok(())
}

pub async fn list(State(state): State<AppState>, auth: Auth, Path(project_id): Path<String>) -> AppResult<Json<Value>> {
    {
        let mut conn = state.db.acquire().await?;
        project_in_workspace(&mut conn, &auth.workspace_id, &project_id).await?;
    }
    Ok(Json(json!({ "controls": out(project_controls(&state, &project_id).await?) })))
}

/// Adds controls in one transaction, numbering them after the highest number
/// ever used in the project. Skips any whose text the project already has.
async fn add(
    state: &AppState,
    auth: &Auth,
    project_id: &str,
    items: Vec<(String, String)>,
    source: Option<(&str, &str)>,
) -> AppResult<(Vec<String>, usize)> {
    let t = now();
    let mut tx = state.db.begin().await?;
    let existing: Vec<(String,)> = sqlx::query_as("SELECT text FROM project_controls WHERE project_id = ? AND archived_at IS NULL")
        .bind(project_id)
        .fetch_all(&mut *tx)
        .await?;
    let mut known: std::collections::BTreeSet<String> = existing.iter().map(|(t,)| controls::normalize(t)).collect();
    let (active,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM project_controls WHERE project_id = ? AND archived_at IS NULL")
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let (mut number,): (i64,) = sqlx::query_as("SELECT COALESCE(MAX(number), 0) FROM project_controls WHERE project_id = ?")
        .bind(project_id)
        .fetch_one(&mut *tx)
        .await?;
    let mut added = Vec::new();
    let mut skipped = 0;
    for (section, text) in items {
        if !known.insert(controls::normalize(&text)) {
            skipped += 1;
            continue;
        }
        if active + added.len() as i64 >= MAX_PROJECT_CONTROLS {
            return Err(AppError::bad_request("too_many", format!("A project can have at most {MAX_PROJECT_CONTROLS} controls.")));
        }
        number += 1;
        sqlx::query(
            "INSERT INTO project_controls (id, workspace_id, project_id, number, text, section, source_id, created_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(&auth.workspace_id)
        .bind(project_id)
        .bind(number)
        .bind(&text)
        .bind(&section)
        .bind(source.map(|(id, _)| id))
        .bind(&auth.user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
        added.push(controls::reference(number));
    }
    if !added.is_empty() {
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "controls.added", Some(project_id),
            json!({ "project_id": project_id, "controls": added, "source": source.map(|(id, name)| json!({ "id": id, "filename": name })) }))
        .await?;
    }
    tx.commit().await?;
    Ok((added, skipped))
}

#[derive(Deserialize)]
pub struct NewControl {
    text: String,
    #[serde(default)]
    section: String,
}

pub async fn create(
    State(state): State<AppState>,
    auth: Auth,
    Path(project_id): Path<String>,
    Body(body): Body<NewControl>,
) -> AppResult<(StatusCode, Json<Value>)> {
    writable_project(&state, &auth, &project_id).await?;
    let text = super::projects::clean_text(&body.text, "control", MAX_CONTROL_CHARS, true)?;
    let section = super::projects::clean_text(&body.section, "section", 40, false)?;
    let (added, _) = add(&state, &auth, &project_id, vec![(section, text)], None).await?;
    if added.is_empty() {
        return Err(AppError::conflict("duplicate", "This project already has that control."));
    }
    Ok((StatusCode::CREATED, Json(json!({ "added": added, "controls": out(project_controls(&state, &project_id).await?) }))))
}

/// Finds the "shall" and "must" statements in a source file and adds them.
pub async fn extract(
    State(state): State<AppState>,
    auth: Auth,
    Path((project_id, source_id)): Path<(String, String)>,
) -> AppResult<(StatusCode, Json<Value>)> {
    writable_project(&state, &auth, &project_id).await?;
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT filename, content_type FROM project_sources WHERE id = ? AND project_id = ? AND deleted_at IS NULL",
    )
    .bind(&source_id)
    .bind(&project_id)
    .fetch_optional(&state.db)
    .await?;
    let (filename, content_type) = row.ok_or_else(|| AppError::not_found("No such file."))?;
    let bytes = crate::storage::get(&state, "sources", &source_id).await?.ok_or_else(|| AppError::not_found("The stored file is missing."))?;
    let text = crate::extract::text_of(bytes, content_type)
        .await
        .map_err(|reason| AppError::new(StatusCode::UNPROCESSABLE_ENTITY, "unreadable", format!("Peter could not read {filename}: {reason}.")))?;
    let found = controls::statements(&text);
    if found.is_empty() {
        return Err(AppError::new(StatusCode::UNPROCESSABLE_ENTITY, "no_controls",
            format!("No \"shall\" or \"must\" statements were found in {filename}. Add its controls by hand.")));
    }
    let truncated = found.len() == controls::MAX_STATEMENTS;
    let items = found.into_iter().map(|s| (s.section, s.text.chars().take(MAX_CONTROL_CHARS).collect())).collect();
    let (added, skipped) = add(&state, &auth, &project_id, items, Some((&source_id, &filename))).await?;
    Ok((StatusCode::CREATED, Json(json!({
        "added": added, "skipped": skipped, "truncated": truncated,
        "controls": out(project_controls(&state, &project_id).await?),
    }))))
}

pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path((project_id, control_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    writable_project(&state, &auth, &project_id).await?;
    let mut tx = state.db.begin().await?;
    let row: Option<(i64, String)> =
        sqlx::query_as("SELECT number, text FROM project_controls WHERE id = ? AND project_id = ? AND archived_at IS NULL")
            .bind(&control_id)
            .bind(&project_id)
            .fetch_optional(&mut *tx)
            .await?;
    let (number, text) = row.ok_or_else(|| AppError::not_found("No such control."))?;
    sqlx::query("UPDATE project_controls SET archived_at = ? WHERE id = ?").bind(now()).bind(&control_id).execute(&mut *tx).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "controls.removed", Some(&project_id),
        json!({ "project_id": project_id, "control": controls::reference(number), "text": text })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------------------------------------------------------------------------
// A session's coverage

struct SessionControls {
    title: String,
    archived: bool,
    controls: Vec<Control>,
    decisions: BTreeMap<String, Decision>,
    matrix: trace::Matrix,
    standards: Vec<String>,
}

async fn load(state: &AppState, auth: &Auth, session_id: &str) -> AppResult<SessionControls> {
    let loaded = super::trace::load(state, auth, session_id).await?;
    let (project_id,): (Option<String>,) = sqlx::query_as("SELECT project_id FROM validation_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(&state.db)
        .await?;
    let mut standards: Vec<String> = sqlx::query_as::<_, (String,)>("SELECT standard_id FROM workspace_standards WHERE workspace_id = ?")
        .bind(&auth.workspace_id)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .map(|(id,)| id)
        .collect();
    standards.sort_by_key(|id| crate::standards::CATALOG.iter().position(|s| s.id == id));
    let mut list = controls::from_standards(&standards);
    if let Some(pid) = &project_id {
        for c in project_controls(state, pid).await? {
            let reference = controls::reference(c.number);
            list.push(Control {
                key: controls::project_key(&reference),
                group: "Company SOPs".into(),
                title: if c.section.is_empty() { String::new() } else { format!("Section {}", c.section) },
                text: c.text,
                source: c.source_filename,
                detect: Detect::Id(reference.clone()),
                reference,
            });
        }
    }
    let rows: Vec<(String, String, bool, String)> =
        sqlx::query_as("SELECT control_key, requirements, not_applicable, reason FROM session_controls WHERE session_id = ?")
            .bind(session_id)
            .fetch_all(&state.db)
            .await?;
    let decisions = rows
        .into_iter()
        .map(|(key, reqs, not_applicable, reason)| {
            (key, Decision { requirements: serde_json::from_str(&reqs).unwrap_or_default(), not_applicable, reason })
        })
        .collect();
    Ok(SessionControls {
        title: loaded.title,
        archived: loaded.archived,
        controls: list,
        decisions,
        matrix: trace::build(&loaded.docs),
        standards,
    })
}

pub async fn coverage(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Json<Value>> {
    let s = load(&state, &auth, &session_id).await?;
    let coverage = controls::coverage(&s.controls, &s.decisions, &s.matrix);
    let requirements: Vec<Value> =
        s.matrix.rows.iter().map(|r| json!({ "id": r.requirement.id, "text": r.requirement.text, "status": r.status })).collect();
    Ok(Json(json!({ "coverage": coverage, "requirements": requirements, "standards": s.standards })))
}

#[derive(Deserialize)]
pub struct DecisionBody {
    #[serde(default)]
    requirements: Vec<String>,
    #[serde(default)]
    not_applicable: bool,
    #[serde(default)]
    reason: String,
}

/// Links requirements to a control by hand, or marks it not applicable.
pub async fn decide(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, key)): Path<(String, String)>,
    Body(body): Body<DecisionBody>,
) -> AppResult<Json<Value>> {
    auth.require(Role::Member)?;
    let s = load(&state, &auth, &session_id).await?;
    if s.archived {
        return Err(AppError::conflict("archived", "This session is archived. Restore it first."));
    }
    let control = s.controls.iter().find(|c| c.key == key).ok_or_else(|| AppError::not_found("No such control in this session."))?;

    let mut requirements: Vec<String> = Vec::new();
    for raw in &body.requirements {
        let found = trace::ids(raw, "URS");
        if found.len() != 1 || found[0] != raw.trim() {
            return Err(AppError::bad_request("invalid_requirement", format!("\"{}\" is not a requirement ID like URS-001.", raw.trim())));
        }
        if !requirements.contains(&found[0]) {
            requirements.push(found[0].clone());
        }
    }
    if requirements.len() > 50 {
        return Err(AppError::bad_request("too_many", "Link at most 50 requirements to a control."));
    }
    let reason = super::projects::clean_text(&body.reason, "reason", 500, body.not_applicable)?;
    let reason = if body.not_applicable { reason } else { String::new() };

    let t = now();
    let mut tx = state.db.begin().await?;
    if requirements.is_empty() && !body.not_applicable {
        sqlx::query("DELETE FROM session_controls WHERE session_id = ? AND control_key = ?")
            .bind(&session_id)
            .bind(&key)
            .execute(&mut *tx)
            .await?;
    } else {
        sqlx::query(
            "INSERT INTO session_controls (session_id, control_key, requirements, not_applicable, reason, updated_by, updated_at)
             VALUES (?, ?, ?, ?, ?, ?, ?)
             ON CONFLICT (session_id, control_key) DO UPDATE SET requirements = excluded.requirements,
             not_applicable = excluded.not_applicable, reason = excluded.reason, updated_by = excluded.updated_by,
             updated_at = excluded.updated_at",
        )
        .bind(&session_id)
        .bind(&key)
        .bind(serde_json::to_string(&requirements).unwrap_or_else(|_| "[]".into()))
        .bind(body.not_applicable)
        .bind(&reason)
        .bind(&auth.user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "control.decided", Some(&session_id),
        json!({ "session_id": session_id, "control": format!("{} {}", control.group, control.reference),
                "requirements": requirements, "not_applicable": body.not_applicable, "reason": reason })).await?;
    tx.commit().await?;

    let _ = state.events.send(SessionEvent {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.clone(),
        name: "controls",
        data: json!({ "key": key }),
    });
    Ok(Json(json!({ "ok": true })))
}

/// The coverage as an Excel workbook.
pub async fn export_xlsx(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Response> {
    let s = load(&state, &auth, &session_id).await?;
    if s.controls.is_empty() {
        return Err(AppError::new(StatusCode::UNPROCESSABLE_ENTITY, "no_controls",
            "There are no controls yet: choose regulations in the Library or add the project's SOP controls."));
    }
    let cov = controls::coverage(&s.controls, &s.decisions, &s.matrix);
    use export::xlsx::Sheet;
    let sheet = Sheet {
        name: "Controls".into(),
        head: ["Source", "Control", "Title", "Statement", "Requirements", "Tests", "Status", "Reason (not applicable)"].map(String::from).to_vec(),
        rows: cov
            .rows
            .iter()
            .map(|r| {
                let mut reqs = r.found.clone();
                reqs.extend(r.linked.iter().map(|id| format!("{id} (linked by hand)")));
                vec![
                    r.source.clone().map(|f| format!("{} ({f})", r.group)).unwrap_or_else(|| r.group.clone()),
                    r.reference.clone(),
                    r.title.clone(),
                    r.text.clone(),
                    reqs.join(", "),
                    r.tests.join(", "),
                    controls::status_label(r.status).into(),
                    r.reason.clone(),
                ]
            })
            .collect(),
    };
    let mut notes: Vec<Vec<String>> = cov.warnings.iter().map(|w| vec![w.clone()]).collect();
    notes.push(vec!["Regulation summaries are Peter's own wording, not the official text.".into()]);
    let notes = Sheet { name: "Notes".into(), head: vec!["Note".into()], rows: notes };
    let bytes = export::xlsx::write(&[sheet, notes], "Controls coverage").map_err(|_| AppError::internal())?;
    let mut tx = state.db.begin().await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.exported", Some(&session_id),
        json!({ "format": "xlsx", "source": "controls" })).await?;
    tx.commit().await?;
    super::review::attachment(
        bytes,
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        &export::file_name(&format!("{} controls", s.title), "xlsx"),
    )
}
