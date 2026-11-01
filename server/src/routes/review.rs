//! M5: comments on documents, signatures on versions, and exports.

use std::io::{Cursor, Write};

use axum::{
    Json,
    body::Body as HttpBody,
    extract::{Path, Query, State},
    http::{StatusCode, header},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

use super::{Body, auth::verify_password};
use crate::{
    auth::{Auth, audit},
    catalog,
    error::{AppError, AppResult},
    export,
    security::{Role, new_id, now},
    state::{AppState, SessionEvent},
};

/// The document for a step, checking the session is in the caller's workspace.
async fn document(state: &AppState, auth: &Auth, session_id: &str, step_id: &str) -> AppResult<(String, String, i64, Option<i64>)> {
    let row: Option<(String, String, i64, Option<i64>)> = sqlx::query_as(
        "SELECT d.id, d.status, d.current_version, s.archived_at FROM documents d
         JOIN validation_sessions s ON s.id = d.session_id
         WHERE d.session_id = ? AND d.step_id = ? AND s.workspace_id = ?",
    )
    .bind(session_id)
    .bind(step_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or_else(|| AppError::not_found("Nothing has been drafted for this step yet."))
}

fn publish(state: &AppState, auth: &Auth, session_id: &str, name: &'static str, data: Value) {
    let _ = state.events.send(SessionEvent {
        workspace_id: auth.workspace_id.clone(),
        session_id: session_id.to_string(),
        name,
        data,
    });
}

// ---------- Comments ----------

#[derive(Serialize, sqlx::FromRow)]
struct CommentView {
    id: String,
    parent_id: Option<String>,
    version: i64,
    quote: String,
    body: String,
    author_id: String,
    author_name: String,
    created_at: i64,
    resolved_at: Option<i64>,
    resolved_by_name: Option<String>,
}

pub async fn list_comments(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
) -> AppResult<Json<Value>> {
    let (doc_id, ..) = document(&state, &auth, &session_id, &step_id).await?;
    let comments: Vec<CommentView> = sqlx::query_as(
        "SELECT c.id, c.parent_id, c.version, c.quote, c.body, c.author_id, a.name AS author_name, c.created_at,
                c.resolved_at, r.name AS resolved_by_name
         FROM document_comments c JOIN users a ON a.id = c.author_id LEFT JOIN users r ON r.id = c.resolved_by
         WHERE c.document_id = ? ORDER BY c.created_at, c.rowid",
    )
    .bind(&doc_id)
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({ "comments": comments })))
}

#[derive(Deserialize)]
pub struct CommentBody {
    body: String,
    #[serde(default)]
    quote: String,
    parent_id: Option<String>,
}

/// Anyone in the workspace may comment, viewers included: a comment does not
/// change the document.
pub async fn add_comment(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Body(body): Body<CommentBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    let (doc_id, _, version, archived) = document(&state, &auth, &session_id, &step_id).await?;
    if archived.is_some() {
        return Err(AppError::conflict("archived", "This session is archived."));
    }
    let text = body.body.trim();
    if text.is_empty() || text.chars().count() > 4000 {
        return Err(AppError::bad_request("invalid_field", "A comment needs 1 to 4000 characters."));
    }
    let quote: String = body.quote.trim().chars().take(1000).collect();
    let mut tx = state.db.begin().await?;
    if let Some(parent) = &body.parent_id {
        let ok: Option<(Option<String>,)> = sqlx::query_as("SELECT parent_id FROM document_comments WHERE id = ? AND document_id = ?")
            .bind(parent)
            .bind(&doc_id)
            .fetch_optional(&mut *tx)
            .await?;
        match ok {
            Some((None,)) => {}
            Some((Some(_),)) => return Err(AppError::bad_request("invalid_parent", "Reply to the comment that opened the thread.")),
            None => return Err(AppError::not_found("No such comment.")),
        }
    }
    let id = new_id();
    sqlx::query(
        "INSERT INTO document_comments (id, workspace_id, document_id, parent_id, version, quote, body, author_id, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&doc_id)
    .bind(&body.parent_id)
    .bind(version)
    .bind(&quote)
    .bind(text)
    .bind(&auth.user_id)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), if body.parent_id.is_some() { "comment.replied" } else { "comment.added" },
        Some(&id), json!({ "session_id": session_id, "step_id": step_id, "version": version })).await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "comment", json!({ "step_id": step_id }));
    Ok((StatusCode::CREATED, Json(json!({ "id": id }))))
}

async fn set_resolved(state: AppState, auth: Auth, comment_id: String, resolve: bool) -> AppResult<StatusCode> {
    #[derive(sqlx::FromRow)]
    struct Row {
        author_id: String,
        parent_id: Option<String>,
        session_id: String,
        resolved_at: Option<i64>,
        step_id: String,
    }
    let row: Option<Row> = sqlx::query_as(
        "SELECT c.author_id, c.parent_id, d.session_id, c.resolved_at, d.step_id
         FROM document_comments c JOIN documents d ON d.id = c.document_id WHERE c.id = ? AND c.workspace_id = ?",
    )
    .bind(&comment_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    let Row { author_id: author, parent_id: parent, session_id, resolved_at, step_id } =
        row.ok_or_else(|| AppError::not_found("No such comment."))?;
    if parent.is_some() {
        return Err(AppError::bad_request("not_a_thread", "Resolve the thread, not a reply."));
    }
    // The person who raised it, or anyone who can change documents.
    if author != auth.user_id {
        auth.require(Role::Member)?;
    }
    if resolved_at.is_some() == resolve {
        return Ok(StatusCode::NO_CONTENT);
    }
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE document_comments SET resolved_at = ?, resolved_by = ? WHERE id = ?")
        .bind(if resolve { Some(now()) } else { None })
        .bind(if resolve { Some(&auth.user_id) } else { None })
        .bind(&comment_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), if resolve { "comment.resolved" } else { "comment.reopened" },
        Some(&comment_id), json!({ "session_id": session_id, "step_id": step_id })).await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "comment", json!({ "step_id": step_id }));
    Ok(StatusCode::NO_CONTENT)
}

pub async fn resolve_comment(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<StatusCode> {
    set_resolved(state, auth, id, true).await
}

pub async fn reopen_comment(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<StatusCode> {
    set_resolved(state, auth, id, false).await
}

// ---------- Signatures ----------

#[derive(Deserialize)]
pub struct SignBody {
    version: i64,
    meaning: String,
    password: String,
}

/// Signs the current version. The signer re-enters their password, and their
/// name, the meaning and the time are recorded with the version.
pub async fn sign(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Body(body): Body<SignBody>,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    if !["reviewed", "approved"].contains(&body.meaning.as_str()) {
        return Err(AppError::bad_request("invalid_meaning", "The meaning must be reviewed or approved."));
    }
    let (doc_id, status, current, archived) = document(&state, &auth, &session_id, &step_id).await?;
    if archived.is_some() {
        return Err(AppError::conflict("archived", "This session is archived."));
    }
    if status == "drafting" {
        return Err(AppError::conflict("drafting", "Wait for the draft being written to finish."));
    }
    if body.version != current || current == 0 {
        return Err(AppError::conflict("stale", "Only the current version can be signed. Reload to see it."));
    }
    let (name, email, hash): (String, String, String) = sqlx::query_as("SELECT name, email, password_hash FROM users WHERE id = ?")
        .bind(&auth.user_id)
        .fetch_one(&state.db)
        .await?;
    // Not 401: the person is signed in; only this confirmation failed.
    let key = crate::throttle::user_key(&auth.user_id);
    crate::throttle::check(&state.db, &key).await?;
    if !verify_password(body.password, hash).await? {
        let wrong = AppError::bad_request("wrong_password", "That password is not right. The version was not signed.");
        return Err(crate::throttle::fail(&state.db, &key).await?.unwrap_or(wrong));
    }
    crate::throttle::clear(&state.db, &key).await?;
    let mut tx = state.db.begin().await?;
    let id = new_id();
    let t = now();
    let inserted = sqlx::query(
        "INSERT INTO document_signatures (id, document_id, version, user_id, signer_name, signer_email, meaning, signed_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?) ON CONFLICT (document_id, version, user_id, meaning) DO NOTHING",
    )
    .bind(&id)
    .bind(&doc_id)
    .bind(current)
    .bind(&auth.user_id)
    .bind(&name)
    .bind(&email)
    .bind(&body.meaning)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    if inserted.rows_affected() == 0 {
        return Err(AppError::conflict("already_signed", "You have already signed this version with that meaning."));
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.signed", Some(&doc_id),
        json!({ "session_id": session_id, "step_id": step_id, "version": current, "meaning": body.meaning })).await?;
    tx.commit().await?;
    publish(&state, &auth, &session_id, "document", json!({ "step_id": step_id, "status": status }));
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "version": current, "meaning": body.meaning, "signed_at": t }))))
}

// ---------- Exports ----------

#[derive(Deserialize)]
pub struct ExportQuery {
    version: Option<i64>,
}

pub(super) fn attachment(bytes: Vec<u8>, content_type: &str, filename: &str) -> AppResult<Response> {
    let ascii: String = filename.chars().map(|c| if c.is_ascii_alphanumeric() || ".-_ ".contains(c) { c } else { '_' }).collect();
    let encoded: String = filename
        .bytes()
        .map(|b| if b.is_ascii_alphanumeric() || b".-_".contains(&b) { (b as char).to_string() } else { format!("%{b:02X}") })
        .collect();
    Response::builder()
        .header(header::CONTENT_TYPE, content_type)
        .header(header::CONTENT_DISPOSITION, format!("attachment; filename=\"{ascii}\"; filename*=UTF-8''{encoded}"))
        .header("X-Content-Type-Options", "nosniff")
        .body(HttpBody::from(bytes))
        .map_err(|_| AppError::internal())
}

async fn user_name(state: &AppState, auth: &Auth) -> AppResult<String> {
    let (name,): (String,) = sqlx::query_as("SELECT name FROM users WHERE id = ?").bind(&auth.user_id).fetch_one(&state.db).await?;
    Ok(name)
}

async fn audit_export(state: &AppState, auth: &Auth, target: &str, detail: Value) -> AppResult<()> {
    let mut tx = state.db.begin().await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "document.exported", Some(target), detail).await?;
    tx.commit().await?;
    Ok(())
}

const DOCX: &str = "application/vnd.openxmlformats-officedocument.wordprocessingml.document";
const XLSX: &str = "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet";

pub async fn export_docx(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> AppResult<Response> {
    let name = user_name(&state, &auth).await?;
    let doc = export::load(&state, &auth.workspace_id, &name, &session_id, &step_id, q.version, true).await?;
    let bytes = export::docx::write(&doc).map_err(|e| {
        tracing::error!(error = %e, "Word export failed");
        AppError::internal()
    })?;
    audit_export(&state, &auth, &session_id, json!({ "step_id": step_id, "version": doc.version, "format": "docx" })).await?;
    attachment(bytes, DOCX, &export::file_name(&format!("{} v{}", doc.title, doc.version), "docx"))
}

pub async fn export_xlsx(
    State(state): State<AppState>,
    auth: Auth,
    Path((session_id, step_id)): Path<(String, String)>,
    Query(q): Query<ExportQuery>,
) -> AppResult<Response> {
    let name = user_name(&state, &auth).await?;
    let doc = export::load(&state, &auth.workspace_id, &name, &session_id, &step_id, q.version, false).await?;
    let sheets = export::xlsx::tables(&doc.body);
    if sheets.is_empty() {
        return Err(AppError::new(StatusCode::UNPROCESSABLE_ENTITY, "no_tables", "This version has no tables to export to Excel."));
    }
    let bytes = export::xlsx::write(&sheets, &doc.title).map_err(|_| AppError::internal())?;
    audit_export(&state, &auth, &session_id, json!({ "step_id": step_id, "version": doc.version, "format": "xlsx" })).await?;
    attachment(bytes, XLSX, &export::file_name(&format!("{} v{} tables", doc.title, doc.version), "xlsx"))
}

fn csv_field(s: &str) -> String {
    // Quote every field; also defuse cells a spreadsheet would run as formulas.
    let s = if s.starts_with(['=', '+', '-', '@']) { format!("'{s}") } else { s.to_string() };
    format!("\"{}\"", s.replace('"', "\"\""))
}

/// A ZIP of the whole session: every drafted document as Word (and Excel
/// where it has tables), the observed steps, the source files list, and an index.
pub async fn export_package(State(state): State<AppState>, auth: Auth, Path(session_id): Path<String>) -> AppResult<Response> {
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT title, validation_type FROM validation_sessions WHERE id = ? AND workspace_id = ?")
            .bind(&session_id)
            .bind(&auth.workspace_id)
            .fetch_optional(&state.db)
            .await?;
    let (title, type_id) = row.ok_or_else(|| AppError::not_found("No such session."))?;
    let kind = catalog::find(&type_id).ok_or_else(AppError::internal)?;
    let name = user_name(&state, &auth).await?;
    let drafted: Vec<(String,)> =
        sqlx::query_as("SELECT step_id FROM documents WHERE session_id = ? AND current_version > 0").bind(&session_id).fetch_all(&state.db).await?;

    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
    let zip_err = |e: zip::result::ZipError| {
        tracing::error!(error = %e, "package export failed");
        AppError::internal()
    };
    let mut index = format!("{title}\n{}\nExported from Peter by {name} on {}\n\nDocuments\n", kind.title, export::utc(now()));
    let mut count = 0;
    for (n, step) in kind.steps.iter().enumerate() {
        if !drafted.iter().any(|(s,)| s == step.id) {
            continue;
        }
        let doc = export::load(&state, &auth.workspace_id, &name, &session_id, step.id, None, true).await?;
        let base = format!("{:02} {}", n + 1, doc.title);
        let docx = export::docx::write(&doc).map_err(|_| AppError::internal())?;
        zip.start_file(format!("documents/{}", export::file_name(&base, "docx")), opts).map_err(zip_err)?;
        zip.write_all(&docx).map_err(|_| AppError::internal())?;
        let sheets = export::xlsx::tables(&doc.body);
        if !sheets.is_empty() {
            let xlsx = export::xlsx::write(&sheets, &doc.title).map_err(|_| AppError::internal())?;
            zip.start_file(format!("documents/{}", export::file_name(&format!("{base} tables"), "xlsx")), opts).map_err(zip_err)?;
            zip.write_all(&xlsx).map_err(|_| AppError::internal())?;
        }
        let signed = if doc.signatures.is_empty() {
            "not signed".to_string()
        } else {
            doc.signatures.iter().map(|s| format!("{} by {}", s.meaning.to_lowercase(), s.name)).collect::<Vec<_>>().join(", ")
        };
        index.push_str(&format!("- {}: version {}, {}; {} open gap(s)\n", doc.title, doc.version, signed, doc.gaps));
        count += 1;
    }
    if count == 0 {
        index.push_str("(none drafted yet)\n");
    }

    // Every observed step, for reference outside Peter.
    #[derive(sqlx::FromRow)]
    struct Obs {
        seq: i64,
        kind: String,
        url: String,
        page_title: String,
        target_label: String,
        value: String,
        masked: bool,
        note: String,
        captured_at_ms: i64,
        by: String,
    }
    let rows: Vec<Obs> = sqlx::query_as(
        "SELECT o.seq, o.kind, o.url, o.page_title, o.target_label, o.value, o.masked, o.note, o.captured_at_ms, u.name AS by
         FROM observations o JOIN users u ON u.id = o.created_by WHERE o.session_id = ? AND o.deleted_at IS NULL ORDER BY o.seq",
    )
    .bind(&session_id)
    .fetch_all(&state.db)
    .await?;
    let mut csv = String::from("ref,kind,page,url,element,value,note,by,when_utc\r\n");
    for o in &rows {
        let value = if o.masked { "(hidden)" } else { o.value.as_str() };
        let fields = [
            format!("O{}", o.seq), o.kind.clone(), o.page_title.clone(), o.url.clone(), o.target_label.clone(),
            value.to_string(), o.note.clone(), o.by.clone(), export::utc(o.captured_at_ms / 1000),
        ];
        csv.push_str(&fields.iter().map(|f| csv_field(f)).collect::<Vec<_>>().join(","));
        csv.push_str("\r\n");
    }
    zip.start_file("evidence/observed-steps.csv", opts).map_err(zip_err)?;
    zip.write_all(csv.as_bytes()).map_err(|_| AppError::internal())?;
    index.push_str(&format!("\nEvidence\n- evidence/observed-steps.csv: {} observed step(s)\n", rows.len()));

    zip.start_file("README.txt", opts).map_err(zip_err)?;
    zip.write_all(index.replace('\n', "\r\n").as_bytes()).map_err(|_| AppError::internal())?;
    let bytes = zip.finish().map_err(zip_err)?.into_inner();

    audit_export(&state, &auth, &session_id, json!({ "format": "zip", "documents": count })).await?;
    attachment(bytes, "application/zip", &export::file_name(&format!("{title} validation package"), "zip"))
}
