//! The Library: regulations in scope, company templates, and branding.

use axum::{
    Json,
    body::Body as HttpBody,
    extract::{Multipart, Path, State},
    http::{StatusCode, header},
    response::Response,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use super::{Body, projects::clean_text};
use crate::{
    auth::{Auth, audit},
    catalog,
    error::{AppError, AppResult},
    extract, prompts,
    security::{Role, new_id, now},
    standards,
    state::AppState,
};

const MAX_TEMPLATE_BYTES: usize = 10 * 1024 * 1024;
pub const MAX_LOGO_BYTES: usize = 1024 * 1024;

// ---------- Regulations ----------

pub async fn list_standards(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let chosen: Vec<(String,)> = sqlx::query_as("SELECT standard_id FROM workspace_standards WHERE workspace_id = ?")
        .bind(&auth.workspace_id)
        .fetch_all(&state.db)
        .await?;
    let selected: Vec<String> = chosen.into_iter().map(|(id,)| id).collect();
    Ok(Json(json!({ "standards": standards::CATALOG, "selected": selected })))
}

#[derive(Deserialize)]
pub struct StandardsBody {
    selected: Vec<String>,
}

pub async fn set_standards(State(state): State<AppState>, auth: Auth, Body(body): Body<StandardsBody>) -> AppResult<Json<Value>> {
    auth.require(Role::Admin)?;
    if let Some(bad) = body.selected.iter().find(|id| standards::find(id).is_none()) {
        return Err(AppError::bad_request("unknown_standard", format!("Peter does not know the regulation \"{bad}\".")));
    }
    let mut selected = body.selected.clone();
    selected.sort();
    selected.dedup();
    let mut tx = state.db.begin().await?;
    sqlx::query("DELETE FROM workspace_standards WHERE workspace_id = ?").bind(&auth.workspace_id).execute(&mut *tx).await?;
    for id in &selected {
        sqlx::query("INSERT INTO workspace_standards (workspace_id, standard_id) VALUES (?, ?)")
            .bind(&auth.workspace_id)
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.standards_changed", None, json!({ "selected": selected })).await?;
    tx.commit().await?;
    Ok(Json(json!({ "selected": selected })))
}

// ---------- Templates ----------

#[derive(Serialize, sqlx::FromRow)]
struct TemplateView {
    id: String,
    step_id: String,
    name: String,
    filename: String,
    sha256: String,
    chars: i64,
    created_by_name: String,
    created_at: i64,
}

pub async fn list_templates(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Value>> {
    let templates: Vec<TemplateView> = sqlx::query_as(
        "SELECT t.id, t.step_id, t.name, t.filename, t.sha256, LENGTH(t.text) AS chars, u.name AS created_by_name, t.created_at
         FROM library_templates t JOIN users u ON u.id = t.created_by
         WHERE t.workspace_id = ? AND t.archived_at IS NULL ORDER BY t.step_id",
    )
    .bind(&auth.workspace_id)
    .fetch_all(&state.db)
    .await?;
    // The steps that draft a document, so the page can offer a template for each.
    let steps: Vec<Value> = catalog::TYPES
        .iter()
        .filter(|t| t.available)
        .flat_map(|t| t.steps.iter().map(move |s| (t, s)))
        .filter_map(|(t, s)| {
            prompts::spec(s.id).map(|spec| json!({ "type": t.title, "step_id": s.id, "step": s.label, "document": spec.title }))
        })
        .collect();
    Ok(Json(json!({ "templates": templates, "steps": steps })))
}

pub async fn get_template(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<Json<Value>> {
    let row: Option<(String, String, String)> = sqlx::query_as(
        "SELECT name, step_id, text FROM library_templates WHERE id = ? AND workspace_id = ? AND archived_at IS NULL",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .fetch_optional(&state.db)
    .await?;
    let (name, step_id, text) = row.ok_or_else(|| AppError::not_found("No such template."))?;
    Ok(Json(json!({ "id": id, "name": name, "step_id": step_id, "text": text })))
}

/// Reads one multipart upload: the file's name and bytes, plus other text fields.
pub(super) async fn read_upload(mut multipart: Multipart, max: usize) -> AppResult<(String, Vec<u8>, std::collections::HashMap<String, String>)> {
    let bad = |m: String| AppError::bad_request("invalid_upload", m);
    let mut fields = std::collections::HashMap::new();
    let mut file: Option<(String, Vec<u8>)> = None;
    while let Some(mut field) = multipart.next_field().await.map_err(|e| bad(e.body_text()))? {
        let name = field.name().unwrap_or("").to_string();
        if name == "file" {
            let filename = field.file_name().unwrap_or("").rsplit(['/', '\\']).next().unwrap_or("").trim().to_string();
            let mut bytes = Vec::new();
            while let Some(chunk) = field.chunk().await.map_err(|e| bad(e.body_text()))? {
                bytes.extend_from_slice(&chunk);
                if bytes.len() > max {
                    return Err(AppError::new(StatusCode::PAYLOAD_TOO_LARGE, "too_large", format!("Files can be at most {} MB.", max / (1024 * 1024))));
                }
            }
            file = Some((filename, bytes));
        } else {
            fields.insert(name, field.text().await.map_err(|e| bad(e.body_text()))?);
        }
    }
    let (filename, bytes) = file.ok_or_else(|| bad("Send the file in a form field named \"file\".".into()))?;
    if bytes.is_empty() {
        return Err(bad("The file is empty.".into()));
    }
    Ok((filename, bytes, fields))
}

/// Adds a company template for a step, replacing the current one for that step.
pub async fn upload_template(State(state): State<AppState>, auth: Auth, multipart: Multipart) -> AppResult<(StatusCode, Json<Value>)> {
    auth.require(Role::Admin)?;
    let (filename, bytes, fields) = read_upload(multipart, MAX_TEMPLATE_BYTES).await?;
    let step_id = fields.get("step_id").cloned().unwrap_or_default();
    if prompts::spec(&step_id).is_none() {
        return Err(AppError::bad_request("unknown_step", "Choose a step that drafts a document."));
    }
    let ext = filename.rsplit_once('.').map(|(_, e)| e.to_ascii_lowercase()).unwrap_or_default();
    let content_type = match ext.as_str() {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pdf" => "application/pdf",
        "txt" => "text/plain",
        "md" => "text/markdown",
        _ => return Err(AppError::bad_request("file_type", "Upload the template as Word (.docx), PDF, text or Markdown.")),
    };
    let name = clean_text(fields.get("name").map(String::as_str).unwrap_or(&filename), "template name", 120, true)?;

    let text = extract::text_of(bytes.clone(), content_type.to_string()).await;
    let text = extract::tidy(&text.map_err(|reason| AppError::bad_request("unreadable", format!("Peter could not read the template: {reason}.")))?);
    if text.chars().count() < 20 {
        return Err(AppError::bad_request("unreadable", "The template has almost no text Peter can read."));
    }
    let sha = hex::encode(Sha256::digest(&bytes));

    let (id, t) = (new_id(), now());
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE library_templates SET archived_at = ? WHERE workspace_id = ? AND step_id = ? AND archived_at IS NULL")
        .bind(t)
        .bind(&auth.workspace_id)
        .bind(&step_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query(
        "INSERT INTO library_templates (id, workspace_id, step_id, name, filename, text, sha256, created_by, created_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(&id)
    .bind(&auth.workspace_id)
    .bind(&step_id)
    .bind(&name)
    .bind(&filename)
    .bind(&text)
    .bind(&sha)
    .bind(&auth.user_id)
    .bind(t)
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.template_added", Some(&id),
        json!({ "step_id": step_id, "name": name, "filename": filename, "sha256": sha })).await?;
    tx.commit().await?;
    Ok((StatusCode::CREATED, Json(json!({ "id": id, "name": name, "step_id": step_id, "chars": text.chars().count() }))))
}

pub async fn remove_template(State(state): State<AppState>, auth: Auth, Path(id): Path<String>) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    let done = sqlx::query("UPDATE library_templates SET archived_at = ? WHERE id = ? AND workspace_id = ? AND archived_at IS NULL")
        .bind(now())
        .bind(&id)
        .bind(&auth.workspace_id)
        .execute(&mut *tx)
        .await?;
    if done.rows_affected() == 0 {
        return Err(AppError::not_found("No such template."));
    }
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.template_removed", Some(&id), json!({})).await?;
    tx.commit().await?;
    Ok(StatusCode::NO_CONTENT)
}

// ---------- Branding ----------

#[derive(Serialize, Default, sqlx::FromRow)]
pub struct Branding {
    pub company_name: String,
    pub doc_prefix: String,
    pub footer_text: String,
    pub logo_type: Option<String>,
}

pub async fn branding_of(db: &sqlx::SqlitePool, workspace_id: &str) -> AppResult<Branding> {
    Ok(sqlx::query_as("SELECT company_name, doc_prefix, footer_text, logo_type FROM workspace_branding WHERE workspace_id = ?")
        .bind(workspace_id)
        .fetch_optional(db)
        .await?
        .unwrap_or_default())
}

pub async fn get_branding(State(state): State<AppState>, auth: Auth) -> AppResult<Json<Branding>> {
    Ok(Json(branding_of(&state.db, &auth.workspace_id).await?))
}

#[derive(Deserialize)]
pub struct BrandingBody {
    company_name: String,
    doc_prefix: String,
    footer_text: String,
}

pub async fn set_branding(State(state): State<AppState>, auth: Auth, Body(body): Body<BrandingBody>) -> AppResult<Json<Branding>> {
    auth.require(Role::Admin)?;
    let company_name = clean_text(&body.company_name, "company name", 120, false)?;
    let footer_text = clean_text(&body.footer_text, "footer text", 200, false)?;
    let doc_prefix = clean_text(&body.doc_prefix, "document number prefix", 20, false)?.to_uppercase();
    if !doc_prefix.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Err(AppError::bad_request("invalid_field", "The document number prefix can use letters, digits, - and _ only."));
    }
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO workspace_branding (workspace_id, company_name, doc_prefix, footer_text, updated_by, updated_at)
         VALUES (?, ?, ?, ?, ?, ?)
         ON CONFLICT (workspace_id) DO UPDATE SET company_name = excluded.company_name, doc_prefix = excluded.doc_prefix,
           footer_text = excluded.footer_text, updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(&auth.workspace_id)
    .bind(&company_name)
    .bind(&doc_prefix)
    .bind(&footer_text)
    .bind(&auth.user_id)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.branding_changed", None,
        json!({ "company_name": company_name, "doc_prefix": doc_prefix, "footer_text": footer_text })).await?;
    tx.commit().await?;
    Ok(Json(branding_of(&state.db, &auth.workspace_id).await?))
}

pub async fn upload_logo(State(state): State<AppState>, auth: Auth, multipart: Multipart) -> AppResult<Json<Branding>> {
    auth.require(Role::Admin)?;
    let (_, bytes, _) = read_upload(multipart, MAX_LOGO_BYTES).await?;
    let kind = if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(&[0xFF, 0xD8, 0xFF]) {
        "image/jpeg"
    } else {
        return Err(AppError::bad_request("file_type", "Upload the logo as a PNG or JPEG image."));
    };
    if crate::export::image_size(&bytes).is_none() {
        return Err(AppError::bad_request("file_type", "Peter could not read that image's size."));
    }
    crate::storage::put(&state, "logos", &auth.workspace_id, &bytes).await?;
    let sha = hex::encode(Sha256::digest(&bytes));
    let mut tx = state.db.begin().await?;
    sqlx::query(
        "INSERT INTO workspace_branding (workspace_id, logo_type, logo_sha256, updated_by, updated_at) VALUES (?, ?, ?, ?, ?)
         ON CONFLICT (workspace_id) DO UPDATE SET logo_type = excluded.logo_type, logo_sha256 = excluded.logo_sha256,
           updated_by = excluded.updated_by, updated_at = excluded.updated_at",
    )
    .bind(&auth.workspace_id)
    .bind(kind)
    .bind(&sha)
    .bind(&auth.user_id)
    .bind(now())
    .execute(&mut *tx)
    .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.logo_changed", None, json!({ "sha256": sha })).await?;
    tx.commit().await?;
    Ok(Json(branding_of(&state.db, &auth.workspace_id).await?))
}

pub async fn remove_logo(State(state): State<AppState>, auth: Auth) -> AppResult<StatusCode> {
    auth.require(Role::Admin)?;
    let mut tx = state.db.begin().await?;
    sqlx::query("UPDATE workspace_branding SET logo_type = NULL, logo_sha256 = NULL, updated_at = ? WHERE workspace_id = ?")
        .bind(now())
        .bind(&auth.workspace_id)
        .execute(&mut *tx)
        .await?;
    audit(&mut tx, Some(&auth.workspace_id), Some(&auth.user_id), "library.logo_changed", None, json!({ "removed": true })).await?;
    tx.commit().await?;
    crate::storage::delete(&state, "logos", &auth.workspace_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

/// The logo's type and bytes, if the workspace has one.
pub async fn logo_of(state: &AppState, workspace_id: &str) -> AppResult<Option<(String, Vec<u8>)>> {
    let Some(kind) = branding_of(&state.db, workspace_id).await?.logo_type else { return Ok(None) };
    Ok(crate::storage::get(state, "logos", workspace_id).await?.map(|b| (kind, b)))
}

pub async fn get_logo(State(state): State<AppState>, auth: Auth) -> AppResult<Response> {
    let (kind, bytes) = logo_of(&state, &auth.workspace_id).await?.ok_or_else(|| AppError::not_found("No logo yet."))?;
    Response::builder()
        .header(header::CONTENT_TYPE, kind)
        .header(header::CACHE_CONTROL, "private, no-cache")
        .header("X-Content-Type-Options", "nosniff")
        .body(HttpBody::from(bytes))
        .map_err(|_| AppError::internal())
}
