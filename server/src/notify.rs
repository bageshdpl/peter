//! Notifications: written in the same transaction as the event they report.

use sqlx::SqliteConnection;

use crate::{
    catalog,
    error::AppResult,
    security::{new_id, now},
};

pub struct Note<'a> {
    pub workspace_id: &'a str,
    pub actor_id: &'a str,
    pub kind: &'a str,
    pub session_id: &'a str,
    pub step_id: Option<&'a str>,
    pub text: String,
}

/// Notifies each person once. Never the person who acted.
pub async fn send(conn: &mut SqliteConnection, to: &[String], note: &Note<'_>) -> AppResult<()> {
    let mut done: Vec<&str> = Vec::new();
    for user in to {
        if user == note.actor_id || done.contains(&user.as_str()) {
            continue;
        }
        done.push(user);
        sqlx::query(
            "INSERT INTO notifications (id, workspace_id, user_id, actor_id, kind, session_id, step_id, text, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id())
        .bind(note.workspace_id)
        .bind(user)
        .bind(note.actor_id)
        .bind(note.kind)
        .bind(note.session_id)
        .bind(note.step_id)
        .bind(&note.text)
        .bind(now())
        .execute(&mut *conn)
        .await?;
    }
    Ok(())
}

/// What a notification calls a place: "User requirements and risk assessment
/// in LIMS 8.2 IV", or just the session's title.
pub async fn place(conn: &mut SqliteConnection, session_id: &str, step_id: Option<&str>) -> AppResult<String> {
    let (title, kind): (String, String) = sqlx::query_as("SELECT title, validation_type FROM validation_sessions WHERE id = ?")
        .bind(session_id)
        .fetch_one(&mut *conn)
        .await?;
    let step = step_id.and_then(|id| catalog::find(&kind).and_then(|k| k.steps.iter().find(|s| s.id == id)).map(|s| s.label));
    Ok(match step {
        Some(label) => format!("“{label}” in {title}"),
        None => format!("“{title}”"),
    })
}

pub async fn name_of(conn: &mut SqliteConnection, user_id: &str) -> AppResult<String> {
    let (name,): (String,) = sqlx::query_as("SELECT name FROM users WHERE id = ?").bind(user_id).fetch_one(&mut *conn).await?;
    Ok(name)
}

/// Members named in a text as "@Their Name" or "@their@email". Names are
/// matched whole, ignoring case, so "@Ana" does not match "Anabel".
pub fn mentions(text: &str, members: &[(String, String, String)]) -> Vec<String> {
    let lower = text.to_lowercase();
    let named = |handle: &str| {
        if handle.trim().is_empty() {
            return false;
        }
        let pat = format!("@{}", handle.to_lowercase());
        let mut from = 0;
        while let Some(pos) = lower[from..].find(&pat).map(|p| p + from) {
            let end = pos + pat.len();
            if lower[end..].chars().next().is_none_or(|c| !c.is_alphanumeric()) {
                return true;
            }
            from = end;
        }
        false
    };
    members.iter().filter(|(_, name, email)| named(name) || named(email)).map(|(id, _, _)| id.clone()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mentions_match_whole_names_and_emails() {
        let m = vec![
            ("u1".to_string(), "Ana Silva".to_string(), "ana@example.com".to_string()),
            ("u2".to_string(), "Ana".to_string(), "a2@example.com".to_string()),
            ("u3".to_string(), "Bo".to_string(), "bo@example.com".to_string()),
        ];
        assert_eq!(mentions("Thanks @ana silva, please check.", &m), ["u1"]);
        assert_eq!(mentions("@Ana: and @bo@example.com", &m), ["u2", "u3"]);
        assert!(mentions("@Anabel and email bo@example.com", &m).is_empty());
    }
}
