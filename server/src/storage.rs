//! Stored files: screenshots, source files and logos. They are kept in the
//! database, so a single backup of it covers everything. Files written by
//! earlier versions under the data folder are still found there.

use crate::{error::AppResult, security::now, state::AppState};

/// A file's place: its kind ("sources", "screenshots", "logos") and id.
pub fn key(kind: &str, id: &str) -> String {
    format!("{kind}/{id}")
}

pub async fn put(state: &AppState, kind: &str, id: &str, data: &[u8]) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO blobs (key, data, created_at) VALUES (?, ?, ?)
         ON CONFLICT (key) DO UPDATE SET data = excluded.data, created_at = excluded.created_at",
    )
    .bind(key(kind, id))
    .bind(data)
    .bind(now())
    .execute(&state.db)
    .await?;
    Ok(())
}

pub async fn get(state: &AppState, kind: &str, id: &str) -> AppResult<Option<Vec<u8>>> {
    let row: Option<(Vec<u8>,)> = sqlx::query_as("SELECT data FROM blobs WHERE key = ?").bind(key(kind, id)).fetch_optional(&state.db).await?;
    if let Some((data,)) = row {
        return Ok(Some(data));
    }
    // Written by an earlier version as a plain file.
    Ok(tokio::fs::read(state.config.data_dir.join(kind).join(id)).await.ok())
}

pub async fn delete(state: &AppState, kind: &str, id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM blobs WHERE key = ?").bind(key(kind, id)).execute(&state.db).await?;
    let _ = tokio::fs::remove_file(state.config.data_dir.join(kind).join(id)).await;
    Ok(())
}
