//! Exporting documents: Word, Excel, and a ZIP package of a whole session.

pub mod docx;
pub mod markdown;
pub mod xlsx;

use std::collections::BTreeMap;

use serde_json::Value;

use crate::{
    catalog,
    error::{AppError, AppResult},
    state::AppState,
};

pub struct Signature {
    pub name: String,
    pub email: String,
    pub meaning: String,
    pub signed_at: i64,
}

pub struct CitedObservation {
    pub seq: i64,
    pub description: String,
    pub by: String,
    pub captured_at_ms: i64,
    pub screenshot: Option<(String, Vec<u8>)>,
    pub screenshot_sha256: String,
}

pub struct CitedSource {
    pub tag: String,
    pub filename: String,
    pub sha256: String,
}

pub struct HistoryRow {
    pub version: i64,
    pub description: String,
    pub by: String,
    pub at: i64,
}

/// Everything an exported document shows.
pub struct ExportDoc {
    pub title: String,
    pub workspace_name: String,
    pub project_name: String,
    pub system_name: String,
    pub session_title: String,
    pub type_title: String,
    pub step_label: String,
    pub version: i64,
    pub version_origin: String,
    pub version_by: String,
    pub version_at: i64,
    pub model: Option<String>,
    pub body: String,
    pub signatures: Vec<Signature>,
    pub observations: Vec<CitedObservation>,
    pub sources: Vec<CitedSource>,
    pub document_titles: BTreeMap<String, String>,
    pub history: Vec<HistoryRow>,
    pub gaps: i64,
    pub unknown: Vec<String>,
    pub exported_by: String,
    pub exported_at: i64,
    pub branding: BrandingOut,
}

/// How the company appears on exports (from the Library's Branding).
#[derive(Default)]
pub struct BrandingOut {
    pub company_name: String,
    pub footer_text: String,
    /// Prefix, session and step, for example "ACME-CSV-7F3A9C21-URS". Empty without a prefix.
    pub doc_number: String,
    pub logo: Option<(String, Vec<u8>)>,
}

/// A short code for each step's document, used in document numbers.
fn step_code(step_id: &str) -> String {
    match step_id {
        "observe" => "OBS".into(),
        "assessment" => "SA".into(),
        "requirements" => "URS".into(),
        "specification" => "FDS".into(),
        "configuration" => "CS".into(),
        "plan" => "VP".into(),
        "installation" => "IQ".into(),
        "testing" => "TS".into(),
        "traceability" => "TM".into(),
        "summary" => "VSR".into(),
        other => other.to_uppercase().replace('_', "-"),
    }
}

async fn branding(state: &AppState, workspace_id: &str, session_id: &str, step_id: &str) -> AppResult<BrandingOut> {
    let row: Option<(String, String, String, Option<String>)> = sqlx::query_as(
        "SELECT company_name, doc_prefix, footer_text, logo_type FROM workspace_branding WHERE workspace_id = ?",
    )
    .bind(workspace_id)
    .fetch_optional(&state.db)
    .await?;
    let Some((company_name, prefix, footer_text, logo_type)) = row else { return Ok(BrandingOut::default()) };
    let logo = match logo_type {
        Some(kind) => tokio::fs::read(state.config.data_dir.join("logos").join(workspace_id)).await.ok().map(|b| (kind, b)),
        None => None,
    };
    let session: String = session_id.chars().filter(|c| c.is_ascii_hexdigit()).take(8).collect::<String>().to_uppercase();
    let doc_number = if prefix.is_empty() { String::new() } else { format!("{prefix}{session}-{}", step_code(step_id)) };
    Ok(BrandingOut { company_name, footer_text, doc_number, logo })
}

pub fn xml(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            // XML 1.0 forbids most control characters.
            c if (c as u32) < 0x20 && !matches!(c, '\t' | '\n' | '\r') => {}
            c => out.push(c),
        }
    }
    out
}

/// "2026-11-01 07:14 UTC" from Unix seconds.
pub fn utc(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days since 1970-01-01 (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02} UTC", rem / 3600, (rem / 60) % 60)
}

/// Pixel size of a JPEG or PNG, read from its header.
pub fn image_size(b: &[u8]) -> Option<(u32, u32)> {
    if b.starts_with(b"\x89PNG\r\n\x1a\n") && b.len() >= 24 {
        let w = u32::from_be_bytes(b[16..20].try_into().ok()?);
        let h = u32::from_be_bytes(b[20..24].try_into().ok()?);
        return Some((w, h));
    }
    if b.starts_with(&[0xFF, 0xD8]) {
        let mut i = 2;
        while i + 9 < b.len() {
            if b[i] != 0xFF {
                i += 1;
                continue;
            }
            let marker = b[i + 1];
            let len = u16::from_be_bytes([b[i + 2], b[i + 3]]) as usize;
            // Start-of-frame markers carry the size (not DHT, JPG or DAC).
            if (0xC0..=0xCF).contains(&marker) && !matches!(marker, 0xC4 | 0xC8 | 0xCC) {
                let h = u16::from_be_bytes([b[i + 5], b[i + 6]]) as u32;
                let w = u16::from_be_bytes([b[i + 7], b[i + 8]]) as u32;
                return (w > 0 && h > 0).then_some((w, h));
            }
            i += 2 + len;
        }
    }
    None
}

/// A safe file name: letters, digits, spaces, dots, dashes and underscores.
pub fn file_name(base: &str, ext: &str) -> String {
    let clean: String = base
        .chars()
        .map(|c| if c.is_alphanumeric() || " .-_".contains(c) { c } else { '-' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let clean: String = clean.trim_matches(['.', '-', ' ']).chars().take(100).collect();
    format!("{}.{ext}", if clean.is_empty() { "document" } else { &clean })
}

/// Gathers one version of a step's document for export. `version` of `None`
/// means the current version.
pub async fn load(
    state: &AppState,
    workspace_id: &str,
    exported_by: &str,
    session_id: &str,
    step_id: &str,
    version: Option<i64>,
    with_screenshots: bool,
) -> AppResult<ExportDoc> {
    #[derive(sqlx::FromRow)]
    struct SessionRow {
        title: String,
        validation_type: String,
        project_name: Option<String>,
        system_name: Option<String>,
        workspace_name: String,
    }
    let session: Option<SessionRow> = sqlx::query_as(
        "SELECT s.title, s.validation_type, p.name AS project_name, p.system_name, w.name AS workspace_name
         FROM validation_sessions s JOIN workspaces w ON w.id = s.workspace_id
         LEFT JOIN projects p ON p.id = s.project_id
         WHERE s.id = ? AND s.workspace_id = ?",
    )
    .bind(session_id)
    .bind(workspace_id)
    .fetch_optional(&state.db)
    .await?;
    let SessionRow { title: session_title, validation_type: type_id, project_name, system_name, workspace_name } =
        session.ok_or_else(|| AppError::not_found("No such session."))?;
    let kind = catalog::find(&type_id).ok_or_else(AppError::internal)?;
    let step_label = kind.steps.iter().find(|s| s.id == step_id).map(|s| s.label).ok_or_else(|| AppError::not_found("No such step."))?;

    let doc: Option<(String, String, i64)> =
        sqlx::query_as("SELECT id, title, current_version FROM documents WHERE session_id = ? AND step_id = ?")
            .bind(session_id)
            .bind(step_id)
            .fetch_optional(&state.db)
            .await?;
    let (doc_id, title, current) = doc.ok_or_else(|| AppError::not_found("Nothing has been drafted for this step yet."))?;
    let version = version.unwrap_or(current);
    #[derive(sqlx::FromRow)]
    struct VersionRow {
        body: String,
        origin: String,
        provider: Option<String>,
        model: Option<String>,
        citations: String,
        by: String,
        created_at: i64,
    }
    let row: Option<VersionRow> = sqlx::query_as(
        "SELECT v.body, v.origin, v.provider, v.model, v.citations, u.name AS by, v.created_at
         FROM document_versions v JOIN users u ON u.id = v.created_by WHERE v.document_id = ? AND v.version = ?",
    )
    .bind(&doc_id)
    .bind(version)
    .fetch_optional(&state.db)
    .await?;
    let VersionRow { body, origin, provider, model, citations, by: version_by, created_at: version_at } =
        row.ok_or_else(|| AppError::not_found("No such version."))?;
    let citations: Value = serde_json::from_str(&citations).unwrap_or(Value::Null);
    let cited: Vec<String> = citations["cited"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default();

    let signatures = sqlx::query_as::<_, (String, String, String, i64)>(
        "SELECT signer_name, signer_email, meaning, signed_at FROM document_signatures
         WHERE document_id = ? AND version = ? ORDER BY signed_at",
    )
    .bind(&doc_id)
    .bind(version)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(name, email, meaning, signed_at)| Signature {
        name,
        email,
        meaning: if meaning == "approved" { "Approved".into() } else { "Reviewed".into() },
        signed_at,
    })
    .collect();

    // Cited observations, in step order, with their screenshots.
    let seqs: Vec<i64> = cited.iter().filter_map(|c| c.strip_prefix('O')?.parse().ok()).collect();
    let mut observations = Vec::new();
    if !seqs.is_empty() {
        #[derive(sqlx::FromRow)]
        struct Row {
            id: String,
            seq: i64,
            kind: String,
            url: String,
            page_title: String,
            target_label: String,
            value: String,
            masked: bool,
            note: String,
            screenshot_type: Option<String>,
            screenshot_sha256: Option<String>,
            captured_at_ms: i64,
            by: String,
        }
        let rows: Vec<Row> = sqlx::query_as(
            "SELECT o.id, o.seq, o.kind, o.url, o.page_title, o.target_label, o.value, o.masked, o.note,
                    o.screenshot_type, o.screenshot_sha256, o.captured_at_ms, u.name AS by
             FROM observations o JOIN users u ON u.id = o.created_by
             WHERE o.session_id = ? AND o.deleted_at IS NULL ORDER BY o.seq",
        )
        .bind(session_id)
        .fetch_all(&state.db)
        .await?;
        for o in rows.into_iter().filter(|o| seqs.contains(&o.seq)) {
            let place = if o.page_title.is_empty() { o.url.clone() } else { o.page_title.clone() };
            let description = match o.kind.as_str() {
                "navigate" => format!("Opened {place}"),
                "click" => format!("Clicked \"{}\" on {place}", o.target_label),
                "input" if o.masked => format!("Entered a hidden value in \"{}\"", o.target_label),
                "input" => format!("Entered \"{}\" in \"{}\"", o.value, o.target_label),
                "select" => format!("Chose \"{}\" in \"{}\"", o.value, o.target_label),
                "submit" => format!("Submitted \"{}\"", o.target_label),
                "note" => format!("Note: {}", o.note),
                _ => format!("Screenshot of {place}"),
            };
            let screenshot = match (&o.screenshot_type, with_screenshots) {
                (Some(t), true) => tokio::fs::read(state.config.data_dir.join("screenshots").join(&o.id)).await.ok().map(|b| (t.clone(), b)),
                _ => None,
            };
            observations.push(CitedObservation {
                seq: o.seq,
                description,
                by: o.by,
                captured_at_ms: o.captured_at_ms,
                screenshot,
                screenshot_sha256: o.screenshot_sha256.unwrap_or_default(),
            });
        }
    }

    // Cited sources, by the numbers this version used.
    let mut sources = Vec::new();
    if let Some(map) = citations["sources"].as_object() {
        for tag in cited.iter().filter(|c| c.starts_with('S')) {
            if let Some(src) = map.get(tag) {
                let id = src["id"].as_str().unwrap_or_default();
                let sha: Option<(String,)> = sqlx::query_as("SELECT sha256 FROM project_sources WHERE id = ?").bind(id).fetch_optional(&state.db).await?;
                sources.push(CitedSource {
                    tag: tag.clone(),
                    filename: src["filename"].as_str().unwrap_or_default().to_string(),
                    sha256: sha.map(|(s,)| s).unwrap_or_default(),
                });
            }
        }
    }

    let document_titles = kind.steps.iter().map(|s| (s.id.to_string(), s.label.to_string())).collect();
    let history = sqlx::query_as::<_, (i64, String, Option<String>, Option<String>, String, i64)>(
        "SELECT v.version, v.origin, v.provider, v.model, u.name, v.created_at
         FROM document_versions v JOIN users u ON u.id = v.created_by WHERE v.document_id = ? AND v.version <= ? ORDER BY v.version",
    )
    .bind(&doc_id)
    .bind(version)
    .fetch_all(&state.db)
    .await?
    .into_iter()
    .map(|(version, origin, provider, model, by, at)| HistoryRow {
        version,
        description: describe_origin(&origin, provider.as_deref(), model.as_deref()),
        by,
        at,
    })
    .collect();

    Ok(ExportDoc {
        title,
        workspace_name,
        project_name: project_name.unwrap_or_default(),
        system_name: system_name.unwrap_or_default(),
        session_title,
        type_title: kind.title.to_string(),
        step_label: step_label.to_string(),
        version,
        version_origin: describe_origin(&origin, provider.as_deref(), model.as_deref()),
        version_by,
        version_at,
        model: match (&provider, &model) {
            (Some(p), Some(m)) if p != "sample" => Some(format!("{p}, {m}")),
            _ => None,
        },
        body,
        signatures,
        observations,
        sources,
        document_titles,
        history,
        gaps: citations["gaps"].as_i64().unwrap_or(0),
        unknown: citations["unknown"].as_array().map(|a| a.iter().filter_map(|v| v.as_str().map(str::to_string)).collect()).unwrap_or_default(),
        exported_by: exported_by.to_string(),
        exported_at: crate::security::now(),
        branding: branding(state, workspace_id, session_id, step_id).await?,
    })
}

fn describe_origin(origin: &str, provider: Option<&str>, model: Option<&str>) -> String {
    match (origin, provider) {
        ("ai", Some("sample")) => "sample draft, no AI".into(),
        ("ai", Some(p)) => format!("AI draft, {p}{}", model.map(|m| format!(" {m}")).unwrap_or_default()),
        ("generated", _) => "built by Peter from the session's documents".into(),
        _ => "edit by a person".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_dates() {
        assert_eq!(utc(0), "1970-01-01 00:00 UTC");
        assert_eq!(utc(1_793_520_840), "2026-11-01 08:14 UTC");
        assert_eq!(utc(951_782_400), "2000-02-29 00:00 UTC");
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(file_name("User requirements / risk: v2", "docx"), "User requirements - risk- v2.docx");
        assert_eq!(file_name("../../etc", "docx"), "etc.docx");
        assert_eq!(file_name("", "xlsx"), "document.xlsx");
    }

    #[test]
    fn image_sizes() {
        let mut png = b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR".to_vec();
        png.extend_from_slice(&640u32.to_be_bytes());
        png.extend_from_slice(&480u32.to_be_bytes());
        assert_eq!(image_size(&png), Some((640, 480)));
        let jpeg = [0xFF, 0xD8, 0xFF, 0xC0, 0x00, 0x11, 0x08, 0x01, 0xE0, 0x02, 0x80, 0x03, 0, 0, 0, 0];
        assert_eq!(image_size(&jpeg), Some((640, 480)));
    }
}
