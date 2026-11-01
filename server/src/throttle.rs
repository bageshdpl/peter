//! Limits on wrong passwords and codes. After `MAX_FAILURES` wrong attempts
//! for one email, further attempts are refused for `LOCK_SECS`, whatever they
//! contain. Unknown emails are counted the same way, so a lock does not show
//! whether an account exists.

use axum::http::StatusCode;
use sqlx::SqlitePool;

use crate::{
    error::{AppError, AppResult},
    security::now,
};

pub const MAX_FAILURES: i64 = 5;
pub const LOCK_SECS: i64 = 15 * 60;

fn locked_error(until: i64) -> AppError {
    let minutes = ((until - now()) as f64 / 60.0).ceil().max(1.0) as i64;
    AppError::new(
        StatusCode::TOO_MANY_REQUESTS,
        "locked",
        format!("Too many wrong attempts. Try again in {minutes} minute{}.", if minutes == 1 { "" } else { "s" }),
    )
}

/// Refuses the attempt if this key is locked.
pub async fn check(db: &SqlitePool, key: &str) -> AppResult<()> {
    let row: Option<(Option<i64>,)> = sqlx::query_as("SELECT locked_until FROM login_throttle WHERE key = ?")
        .bind(key)
        .fetch_optional(db)
        .await?;
    match row.and_then(|(u,)| u) {
        Some(until) if until > now() => Err(locked_error(until)),
        _ => Ok(()),
    }
}

/// Counts a wrong attempt. Returns the lock error if this attempt locked the key.
pub async fn fail(db: &SqlitePool, key: &str) -> AppResult<Option<AppError>> {
    let t = now();
    // A lock that has run out starts a fresh count.
    let (failures, locked_until): (i64, Option<i64>) = sqlx::query_as(
        "INSERT INTO login_throttle (key, failures, locked_until, updated_at) VALUES (?, 1, NULL, ?)
         ON CONFLICT (key) DO UPDATE SET
           failures = CASE WHEN login_throttle.locked_until IS NOT NULL AND login_throttle.locked_until <= excluded.updated_at
                           THEN 1 ELSE login_throttle.failures + 1 END,
           locked_until = CASE WHEN login_throttle.locked_until IS NOT NULL AND login_throttle.locked_until <= excluded.updated_at
                               THEN NULL ELSE login_throttle.locked_until END,
           updated_at = excluded.updated_at
         RETURNING failures, locked_until",
    )
    .bind(key)
    .bind(t)
    .fetch_one(db)
    .await?;
    if failures >= MAX_FAILURES && locked_until.is_none() {
        let until = t + LOCK_SECS;
        sqlx::query("UPDATE login_throttle SET locked_until = ? WHERE key = ?").bind(until).bind(key).execute(db).await?;
        return Ok(Some(locked_error(until)));
    }
    Ok(None)
}

/// Clears the count after a correct attempt.
pub async fn clear(db: &SqlitePool, key: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM login_throttle WHERE key = ?").bind(key).execute(db).await?;
    Ok(())
}

/// The throttle key for a person's own password checks (signing, password change).
pub fn user_key(user_id: &str) -> String {
    format!("user:{user_id}")
}

pub fn email_key(email: &str) -> String {
    format!("email:{email}")
}
