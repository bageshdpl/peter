//! Reference files attached to a project: vendor manuals, SOPs, specifications.

use axum::{
    Json,
    body::Body as HttpBody,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::Response,
};
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::io::AsyncWriteExt;

use super::projects::project_in_workspace;
use crate::{
    auth::{Auth, audit},
    error::{AppError, AppResult},
    security::{Role, new_id, now},
    state::AppState,
};

/// Accepted file types. The stored type comes from the extension, never from
/// what the browser claims.
const ALLOWED: &[(&str, &str)] = &[
    ("pdf", "application/pdf"),
    ("docx", "application/vnd.openxmlformats-officedocument.wordprocessingml.document"),
    ("xlsx", "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"),
    ("csv", "text/csv"),
    ("txt", "text/plain"),
    ("md", "text/markdown"),
    ("png", "image/png"),
    ("jpg", "image/jpeg"),
    ("jpeg", "image/jpeg"),
];

#[derive(Serialize, sqlx::FromRow)]
pub struct SourceView {
    id: String,
    filename: String,
    content_type: String,
    size_bytes: i64,
    sha256: String,
    uploaded_by_name: String,
    created_at: i64,
}

fn clean_filename(raw: &str) -> AppResult<(String, &'static str)> {
    // Keep only the last path segment and drop control characters.
    let base = raw.rsplit(['/', '\\']).next().unwrap_or("").trim();
    let name: String = base.chars().filter(|c| !c.is_control() && *c != '"').take(200).collect();
    let ext = name.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let content_type = ALLOWED
        .iter()
        .find(|(e, _)| *e == ext)
        .map(|(_, t)| *t)
        .ok_or_else(|| AppError::bad_request("file_type", "Upload a PDF, Word, Excel, CSV, text, Markdown, PNG or JPEG file."))?;
    Ok((name, content_type))
}

pub async fn list(State(state): State<AppState>, auth: Auth, Path(project_id): Path<String>) -> AppResult<Json<Value>> {
    let mut conn = state.db.acquire().await?;
    project_in_workspace(&mut conn, &auth.workspace_id, &project_id).await?;
    let sources: Vec<SourceView> = sqlx::query_as(
        "SELECT f.id, f.filename, f.content_type, f.size_bytes, f.sha256, u.name AS uploaded_by_name, f.created_at
         FROM project_sources f JOIN users u ON u.id = f.uploaded_by
         WHERE f.project_id = ? AND f.deleted_at IS NULL ORDER BY f.created_at DESC",
    )
    .bind(&project_id)
    .fetch_all(&mut *conn)
    .await?;
    Ok(Json(json!({ "sources": sources })))
}

pub async fn upload(
    State(state): State<AppState>,
    auth: Auth,
    Path(project_id): Path<String>,
    mut multipart: Multipart,
) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Member)?;
    {
        let mut conn = state.db.acquire().await?;
        if project_in_workspace(&mut conn, &auth.workspace_id, &project_id).await? {
            return Err(AppError::conflict("archived", "Restore this project before adding files."));
        }
    }

    let bad = |m: &str| AppError::bad_request("invalid_upload", m.to_string());
    let mut field = loop {
        match multipart.next_field().await.map_err(|e| bad(&e.body_text()))? {
            Some(f) if f.name() == Some("file") => break f,
            Some(_) => continue,
            None => return Err(bad("Send the file in a form field named \"file\".")),
        }
    };
    let (filename, content_type) = clean_filename(field.file_name().unwrap_or(""))?;

    let id = new_id();
    let path = state.config.data_dir.join("sources").join(&id);
    let mut file = tokio::fs::File::create(&path).await.map_err(|e| {
        tracing::error!(error = %e, "could not create upload file");
        AppError::internal()
    })?;
    let mut hasher = Sha256::new();
    let mut size = 0usize;
    let result: AppResult<()> = async {
        while let Some(chunk) = field.chunk().await.map_err(|e| bad(&e.body_text()))? {
            size += chunk.len();
            if size > state.config.max_upload_bytes {
                return Err(AppError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large",
                    format!("Files can be at most {} MB.", state.config.max_upload_bytes / (1024 * 1024))));
            }
            hasher.update(&chunk);
            file.write_all(&chunk).await.map_err(|_| AppError::internal())?;
        }
        file.flush().await.map_err(|_| AppError::internal())?;
        if size == 0 {
            return Err(bad("The file is empty."));
        }
        Ok(())
    }
    .await;
    if let Err(err) = result {
        drop(file);
        let _ = tokio::fs::remove_file(&path).await;
        return Err(err);
    }
    let sha256 = hex::encode(hasher.finalize());

    let t = now();
    let saved: AppResult<()> = async {
        let mut tx = state.db.begin().await?;
        sqlx::query(
            "INSERT INTO project_sources
             (id, workspace_id, project_id, filename, content_type, size_bytes, sha256, uploaded_by, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&id)
        .bind(&auth.workspace_id)
        .bind(&project_id)
        .bind(&filename)
        .bind(content_type)
        .bind(size as i64)
        .bind(&sha256)
        .bind(&auth.user_id)
        .bind(t)
        .execute(&mut *tx)
        .await?;
        sqlx::query("UPDATE projects SET updated_at = ? WHERE id = ?").bind(t).bind(&project_id).execute(&mut *tx).await?;
        audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "source.uploaded", Some(&id),
            json!({ "project_id": project_id, "filename": filename, "size_bytes": size, "sha256": sha256 })).await?;
        tx.commit().await?;
        Ok(())
    }
    .await;
    if let Err(err) = saved {
        let _ = tokio::fs::remove_file(&path).await;
        return Err(err);
    }

    let source = json!({
        "id": id, "filename": filename, "content_type": content_type, "size_bytes": size, "sha256": sha256, "created_at": t,
    });
    Ok((StatusCode::CREATED, Json(json!({ "source": source }))))
}

async fn find_source(state: &AppState, auth: &Auth, project_id: &str, source_id: &str) -> AppResult<(String, String)> {
    let row: Option<(String, String)> = sqlx::query_as(
        "SELECT filename, content_type FROM project_sources
         WHERE id = ? AND project_id = ? AND workspace_id = ? AND deleted_at IS NULL",
    )
    .bind(source_id)
    .bind(project_id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    row.ok_or_else(|| AppError::not_found("No such file."))
}

pub async fn download(
    State(state): State<AppState>,
    auth: Auth,
    Path((project_id, source_id)): Path<(String, String)>,
) -> AppResult<Response> {
    let (filename, content_type) = find_source(&state, &auth, &project_id, &source_id).await?;
    let bytes = tokio::fs::read(state.config.data_dir.join("sources").join(&source_id)).await.map_err(|e| {
        tracing::error!(error = %e, %source_id, "stored file is missing");
        AppError::internal()
    })?;
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

/// Hides a file from the project. The stored copy is kept as a record.
pub async fn remove(
    State(state): State<AppState>,
    auth: Auth,
    Path((project_id, source_id)): Path<(String, String)>,
) -> AppResult<StatusCode> {
    auth.require(Role::Member)?;
    let (filename, _) = find_source(&state, &auth, &project_id, &source_id).await?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE project_sources SET deleted_at = ? WHERE id = ?").bind(now()).bind(&source_id).execute(&mut *tx).await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "source.removed", Some(&source_id),
        json!({ "project_id": project_id, "filename": filename })).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}
