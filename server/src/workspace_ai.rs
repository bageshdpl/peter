//! Which AI keys a workspace drafts with: its own, or the server's (when the
//! server allows it, up to a monthly limit), and the record of each call.

use std::collections::BTreeMap;

use serde::Serialize;

use crate::{
    ai::{AiConfig, Completion},
    drafting::DraftJob,
    error::AppResult,
    security::{new_id, now},
    state::AppState,
};

pub const PROVIDERS: &[(&str, &str)] = &[("openrouter", "OpenRouter"), ("gemini", "Google Gemini")];

pub struct Effective {
    pub config: AiConfig,
    /// provider -> "workspace" or "server", for providers that have a key.
    pub sources: BTreeMap<&'static str, &'static str>,
    /// Keys that are stored but cannot be read (the secret key changed).
    pub unreadable: Vec<&'static str>,
}

impl Effective {
    pub fn source(&self, provider: &str) -> Option<&'static str> {
        self.sources.get(provider).copied()
    }
}

#[derive(Serialize)]
pub struct ProviderView {
    pub id: &'static str,
    pub name: &'static str,
    pub model: String,
    pub is_ai: bool,
    /// "workspace", "server", or null for the sample drafter.
    pub key_source: Option<&'static str>,
}

impl Effective {
    pub fn providers(&self) -> Vec<ProviderView> {
        let mut out = Vec::new();
        for &(id, name) in PROVIDERS {
            if let Some(source) = self.source(id) {
                let model = if id == "openrouter" { &self.config.openrouter_model } else { &self.config.gemini_model };
                out.push(ProviderView { id, name, model: model.clone(), is_ai: true, key_source: Some(source) });
            }
        }
        if self.config.sample {
            out.push(ProviderView { id: "sample", name: "Sample draft (no AI)", model: "peter-sample".into(), is_ai: false, key_source: None });
        }
        out
    }
}

/// The workspace's own keys first, then the server's if it shares them.
pub async fn effective(state: &AppState, workspace_id: &str) -> Result<Effective, String> {
    let rows: Vec<(String, String, String)> =
        sqlx::query_as("SELECT provider, key_encrypted, model FROM workspace_ai_keys WHERE workspace_id = ?")
            .bind(workspace_id)
            .fetch_all(&state.db)
            .await
            .map_err(|e| format!("Could not read the workspace's AI settings: {e}"))?;
    let server = &state.config.ai;
    let mut config = AiConfig { openrouter_key: None, gemini_key: None, ..server.clone() };
    let mut sources = BTreeMap::new();
    let mut unreadable = Vec::new();
    for &(id, _) in PROVIDERS {
        let own = rows.iter().find(|r| r.0 == id);
        let (key_slot, model_slot, server_key) = match id {
            "openrouter" => (&mut config.openrouter_key, &mut config.openrouter_model, &server.openrouter_key),
            _ => (&mut config.gemini_key, &mut config.gemini_model, &server.gemini_key),
        };
        if let Some((_, encrypted, model)) = own {
            match state.secret.decrypt(encrypted) {
                Ok(key) => {
                    *key_slot = Some(key);
                    if !model.is_empty() {
                        *model_slot = model.clone();
                    }
                    sources.insert(id, "workspace");
                    continue;
                }
                Err(_) => unreadable.push(id),
            }
        }
        if server.shared && server_key.is_some() {
            *key_slot = server_key.clone();
            sources.insert(id, "server");
        }
    }
    Ok(Effective { config, sources, unreadable })
}

/// Seconds at the start of the current month, UTC.
pub fn month_start(t: i64) -> i64 {
    let days = t.div_euclid(86_400);
    // Civil date from days (Howard Hinnant), then back to the 1st.
    let z = days + 719_468;
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    (days - (d - 1)) * 86_400
}

/// Tokens the workspace used on the server's keys this month.
pub async fn shared_tokens_this_month(state: &AppState, workspace_id: &str) -> AppResult<i64> {
    let (n,): (i64,) = sqlx::query_as(
        "SELECT COALESCE(SUM(COALESCE(prompt_tokens, 0) + COALESCE(completion_tokens, 0)), 0) FROM ai_usage
         WHERE workspace_id = ? AND key_source = 'server' AND created_at >= ?",
    )
    .bind(workspace_id)
    .bind(month_start(now()))
    .fetch_one(&state.db)
    .await?;
    Ok(n)
}

/// Keeps a record of an AI call, whether it worked or not.
pub async fn record(state: &AppState, job: &DraftJob, source: &str, result: &Result<Completion, String>) {
    let (model, prompt, completion, cost, error) = match result {
        Ok(c) => (c.model.clone(), c.prompt_tokens, c.completion_tokens, c.cost_usd, None),
        Err(e) => (String::new(), None, None, None, Some(e.chars().take(500).collect::<String>())),
    };
    let saved = sqlx::query(
        "INSERT INTO ai_usage (id, workspace_id, user_id, session_id, step_id, provider, model, key_source,
         prompt_tokens, completion_tokens, cost_usd, ok, error, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
    )
    .bind(new_id())
    .bind(&job.workspace_id)
    .bind(&job.user_id)
    .bind(&job.session_id)
    .bind(&job.step_id)
    .bind(&job.provider)
    .bind(model)
    .bind(source)
    .bind(prompt)
    .bind(completion)
    .bind(cost)
    .bind(result.is_ok())
    .bind(error)
    .bind(now())
    .execute(&state.db)
    .await;
    if let Err(e) = saved {
        tracing::error!(error = %e, "could not record AI usage");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn month_start_is_the_first_at_midnight() {
        // 2026-11-01 10:39 UTC, 2024-02-29 23:59 UTC, 2027-01-30 00:00 UTC.
        assert_eq!(month_start(1_793_529_540), 1_793_491_200);
        assert_eq!(month_start(1_709_251_140), 1_706_745_600);
        assert_eq!(month_start(1_801_267_200), 1_798_761_600);
    }
}
