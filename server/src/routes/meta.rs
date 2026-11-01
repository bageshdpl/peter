//! Public facts about this server, and the extension download.

use std::io::{Cursor, Write};

use axum::{
    Json,
    body::Body as HttpBody,
    extract::State,
    http::header,
    response::Response,
};
use serde_json::{Value, json};
use zip::{ZipWriter, write::SimpleFileOptions};

use crate::{
    error::{AppError, AppResult},
    state::AppState,
};

/// What the sign-in page and the app need to know before anyone signs in.
pub async fn meta(State(state): State<AppState>) -> AppResult<Json<Value>> {
    let signup = if state.config.allow_signup {
        "open"
    } else {
        let (users,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users").fetch_one(&state.db).await?;
        if users == 0 && state.config.admin_email.is_some() { "first_admin" } else { "invite_only" }
    };
    Ok(Json(json!({
        "signup": signup,
        "extension_download": extension_dir(&state).is_some(),
        "version": env!("CARGO_PKG_VERSION"),
    })))
}

fn extension_dir(state: &AppState) -> Option<std::path::PathBuf> {
    state.config.extension_dir.clone().filter(|d| d.join("manifest.json").is_file())
}

/// "https://peter.example.com" from the app address.
fn origin(app_url: &str) -> String {
    let url = app_url.trim_end_matches('/');
    match url.find("://").and_then(|i| url[i + 3..].find('/').map(|j| i + 3 + j)) {
        Some(end) => url[..end].to_string(),
        None => url.to_string(),
    }
}

/// The extension's manifest, also allowed to talk to this server's address.
pub fn manifest_for(manifest: &str, origin: &str) -> Result<String, String> {
    let mut m: Value = serde_json::from_str(manifest).map_err(|e| e.to_string())?;
    let pattern = Value::String(format!("{origin}/*"));
    for path in ["/host_permissions", "/content_scripts/0/matches"] {
        if let Some(Value::Array(items)) = m.pointer_mut(path)
            && !items.contains(&pattern)
        {
            items.insert(0, pattern.clone());
        }
    }
    serde_json::to_string_pretty(&m).map_err(|e| e.to_string())
}

/// The Peter Observer extension as a zip, set up to work with this server.
pub async fn extension_zip(State(state): State<AppState>) -> AppResult<Response> {
    let dir = extension_dir(&state).ok_or_else(|| AppError::not_found("This server does not offer the extension for download."))?;
    let origin = origin(&state.config.app_url);
    let bytes = tokio::task::spawn_blocking(move || -> Result<Vec<u8>, String> {
        let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
        let opts = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Deflated);
        let mut files = Vec::new();
        collect(&dir, &dir, &mut files).map_err(|e| e.to_string())?;
        files.sort();
        for rel in files {
            let path = dir.join(&rel);
            let name = format!("peter-observer/{}", rel.replace('\\', "/"));
            let data = match rel.as_str() {
                "manifest.json" => manifest_for(&std::fs::read_to_string(&path).map_err(|e| e.to_string())?, &origin)?.into_bytes(),
                "config.js" => {
                    let text = std::fs::read_to_string(&path).map_err(|e| e.to_string())?;
                    let line = format!("export const PETER_URL = {};", serde_json::to_string(&origin).unwrap_or_default());
                    text.lines()
                        .map(|l| if l.starts_with("export const PETER_URL") { line.as_str() } else { l })
                        .collect::<Vec<_>>()
                        .join("\n")
                        .into_bytes()
                }
                _ => std::fs::read(&path).map_err(|e| e.to_string())?,
            };
            zip.start_file(name, opts).map_err(|e| e.to_string())?;
            zip.write_all(&data).map_err(|e| e.to_string())?;
        }
        Ok(zip.finish().map_err(|e| e.to_string())?.into_inner())
    })
    .await
    .map_err(|_| AppError::internal())?
    .map_err(|e| {
        tracing::error!(error = %e, "could not package the extension");
        AppError::internal()
    })?;
    Response::builder()
        .header(header::CONTENT_TYPE, "application/zip")
        .header(header::CONTENT_DISPOSITION, "attachment; filename=\"peter-observer.zip\"")
        .body(HttpBody::from(bytes))
        .map_err(|_| AppError::internal())
}

fn collect(root: &std::path::Path, dir: &std::path::Path, out: &mut Vec<String>) -> std::io::Result<()> {
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with('.') || name == "README.md" {
            continue;
        }
        if path.is_dir() {
            collect(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.push(rel.to_string_lossy().into_owned());
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn origin_drops_paths() {
        assert_eq!(origin("https://peter.onrender.com/"), "https://peter.onrender.com");
        assert_eq!(origin("http://localhost:5173"), "http://localhost:5173");
        assert_eq!(origin("https://x.example.com/app/"), "https://x.example.com");
    }

    #[test]
    fn manifest_gains_the_server_origin_once() {
        let m = r#"{"host_permissions":["http://localhost:5173/*"],"content_scripts":[{"matches":["http://localhost:5173/*"],"js":["bridge.js"]}]}"#;
        let out = manifest_for(m, "https://peter.onrender.com").unwrap();
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["host_permissions"][0], "https://peter.onrender.com/*");
        assert_eq!(v["content_scripts"][0]["matches"].as_array().unwrap().len(), 2);
        let again = manifest_for(&out, "https://peter.onrender.com").unwrap();
        assert_eq!(serde_json::from_str::<Value>(&again).unwrap()["host_permissions"].as_array().unwrap().len(), 2);
    }
}
