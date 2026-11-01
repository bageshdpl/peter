//! AI providers. Each turns a system prompt and a user prompt into text.
//! Keys come from the environment and never leave the server.

use std::time::Duration;

use serde::Serialize;
use serde_json::{Value, json};

#[derive(Debug, Clone)]
pub struct AiConfig {
    pub openrouter_key: Option<String>,
    pub openrouter_model: String,
    pub openrouter_base: String,
    pub gemini_key: Option<String>,
    pub gemini_model: String,
    pub gemini_base: String,
    /// Offers the offline sample drafter (no AI), for trying Peter without a key.
    pub sample: bool,
    pub timeout_secs: u64,
    /// Whether workspaces without their own key may use the server's keys.
    pub shared: bool,
    /// The most tokens a workspace may use each month on the server's keys.
    pub shared_monthly_tokens: Option<i64>,
}

impl AiConfig {
    pub fn from_env() -> Self {
        let var = |name: &str| std::env::var(name).ok().filter(|v| !v.trim().is_empty());
        Self {
            openrouter_key: var("OPENROUTER_API_KEY"),
            openrouter_model: var("PETER_OPENROUTER_MODEL").unwrap_or_else(|| "google/gemini-2.5-flash".into()),
            openrouter_base: var("PETER_OPENROUTER_BASE_URL").unwrap_or_else(|| "https://openrouter.ai/api/v1".into()),
            gemini_key: var("GEMINI_API_KEY"),
            gemini_model: var("PETER_GEMINI_MODEL").unwrap_or_else(|| "gemini-2.5-flash".into()),
            gemini_base: var("PETER_GEMINI_BASE_URL")
                .unwrap_or_else(|| "https://generativelanguage.googleapis.com/v1beta".into()),
            sample: var("PETER_AI_SAMPLE").map(|v| v != "false").unwrap_or(true),
            timeout_secs: var("PETER_AI_TIMEOUT_SECS").and_then(|v| v.parse().ok()).unwrap_or(180),
            shared: var("PETER_SHARED_AI").map(|v| v != "false").unwrap_or(true),
            shared_monthly_tokens: var("PETER_SHARED_AI_MONTHLY_TOKENS").and_then(|v| v.parse().ok()).filter(|n: &i64| *n > 0),
        }
    }

    pub fn providers(&self) -> Vec<ProviderInfo> {
        let mut list = Vec::new();
        if self.openrouter_key.is_some() {
            list.push(ProviderInfo { id: "openrouter", name: "OpenRouter", model: self.openrouter_model.clone(), is_ai: true });
        }
        if self.gemini_key.is_some() {
            list.push(ProviderInfo { id: "gemini", name: "Google Gemini", model: self.gemini_model.clone(), is_ai: true });
        }
        if self.sample {
            list.push(ProviderInfo { id: "sample", name: "Sample draft (no AI)", model: "peter-sample".into(), is_ai: false });
        }
        list
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ProviderInfo {
    pub id: &'static str,
    pub name: &'static str,
    pub model: String,
    pub is_ai: bool,
}

#[derive(Debug)]
pub struct Completion {
    pub text: String,
    pub model: String,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    /// What the provider says the call cost, in US dollars (OpenRouter only).
    pub cost_usd: Option<f64>,
}

pub struct Prompt<'a> {
    pub system: &'a str,
    pub user: &'a str,
}

fn client(config: &AiConfig) -> Result<reqwest::Client, String> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs))
        .build()
        .map_err(|e| format!("Could not set up the HTTP client: {e}"))
}

/// The provider's own error message, if it sent one, without anything that
/// could echo a key back.
fn provider_error(status: reqwest::StatusCode, body: &str) -> String {
    let parsed: Option<Value> = serde_json::from_str(body).ok();
    let message = parsed
        .as_ref()
        .and_then(|v| v.pointer("/error/message").and_then(Value::as_str).map(str::to_string))
        .unwrap_or_else(|| body.chars().take(300).collect());
    format!("The provider answered {status}: {message}")
}

pub async fn complete(config: &AiConfig, provider: &str, prompt: Prompt<'_>) -> Result<Completion, String> {
    match provider {
        "openrouter" => openrouter(config, prompt).await,
        "gemini" => gemini(config, prompt).await,
        _ => Err(format!("Unknown AI provider \"{provider}\".")),
    }
}

/// OpenRouter speaks the OpenAI chat-completions format.
async fn openrouter(config: &AiConfig, prompt: Prompt<'_>) -> Result<Completion, String> {
    let key = config.openrouter_key.as_deref().ok_or("OpenRouter has no API key (OPENROUTER_API_KEY).")?;
    let body = json!({
        "model": config.openrouter_model,
        "temperature": 0.2,
        // Asks OpenRouter to report the call's cost.
        "usage": { "include": true },
        "messages": [
            { "role": "system", "content": prompt.system },
            { "role": "user", "content": prompt.user },
        ],
    });
    let res = client(config)?
        .post(format!("{}/chat/completions", config.openrouter_base.trim_end_matches('/')))
        .bearer_auth(key)
        .header("X-Title", "Peter")
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Could not reach OpenRouter: {e}"))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| format!("OpenRouter's answer was cut off: {e}"))?;
    if !status.is_success() {
        return Err(provider_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| "OpenRouter's answer was not JSON.".to_string())?;
    // OpenRouter can return 200 with an error object.
    if let Some(message) = v.pointer("/error/message").and_then(Value::as_str) {
        return Err(format!("OpenRouter: {message}"));
    }
    let content = v
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .ok_or("OpenRouter returned no text.")?;
    Ok(Completion {
        text: content.to_string(),
        model: v.get("model").and_then(Value::as_str).unwrap_or(&config.openrouter_model).to_string(),
        prompt_tokens: v.pointer("/usage/prompt_tokens").and_then(Value::as_i64),
        completion_tokens: v.pointer("/usage/completion_tokens").and_then(Value::as_i64),
        cost_usd: v.pointer("/usage/cost").and_then(Value::as_f64),
    })
}

async fn gemini(config: &AiConfig, prompt: Prompt<'_>) -> Result<Completion, String> {
    let key = config.gemini_key.as_deref().ok_or("Gemini has no API key (GEMINI_API_KEY).")?;
    let body = json!({
        "systemInstruction": { "parts": [{ "text": prompt.system }] },
        "contents": [{ "role": "user", "parts": [{ "text": prompt.user }] }],
        "generationConfig": { "temperature": 0.2 },
    });
    let res = client(config)?
        .post(format!("{}/models/{}:generateContent", config.gemini_base.trim_end_matches('/'), config.gemini_model))
        // In a header, not the URL, so it cannot end up in logs.
        .header("x-goog-api-key", key)
        .json(&body)
        .send()
        .await
        .map_err(|e| format!("Could not reach Gemini: {}", e.without_url()))?;
    let status = res.status();
    let text = res.text().await.map_err(|e| format!("Gemini's answer was cut off: {}", e.without_url()))?;
    if !status.is_success() {
        return Err(provider_error(status, &text));
    }
    let v: Value = serde_json::from_str(&text).map_err(|_| "Gemini's answer was not JSON.".to_string())?;
    let parts = v.pointer("/candidates/0/content/parts").and_then(Value::as_array);
    let content: String = parts
        .map(|ps| ps.iter().filter_map(|p| p.get("text").and_then(Value::as_str)).collect::<Vec<_>>().join(""))
        .unwrap_or_default();
    if content.trim().is_empty() {
        let reason = v
            .pointer("/candidates/0/finishReason")
            .or_else(|| v.pointer("/promptFeedback/blockReason"))
            .and_then(Value::as_str)
            .unwrap_or("no reason given");
        return Err(format!("Gemini returned no text ({reason})."));
    }
    Ok(Completion {
        text: content,
        model: v.get("modelVersion").and_then(Value::as_str).unwrap_or(&config.gemini_model).to_string(),
        prompt_tokens: v.pointer("/usageMetadata/promptTokenCount").and_then(Value::as_i64),
        completion_tokens: v.pointer("/usageMetadata/candidatesTokenCount").and_then(Value::as_i64),
        cost_usd: None,
    })
}

/// Checks a key without spending anything: OpenRouter's key endpoint, or
/// Gemini's model list. Returns a short description of what was found.
pub async fn check_key(config: &AiConfig, provider: &str) -> Result<String, String> {
    let http = client(config)?;
    match provider {
        "openrouter" => {
            let key = config.openrouter_key.as_deref().ok_or("No OpenRouter key.")?;
            let res = http
                .get(format!("{}/key", config.openrouter_base.trim_end_matches('/')))
                .bearer_auth(key)
                .send()
                .await
                .map_err(|e| format!("Could not reach OpenRouter: {e}"))?;
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(provider_error(status, &text));
            }
            let v: Value = serde_json::from_str(&text).unwrap_or_default();
            let usage = v.pointer("/data/usage").and_then(Value::as_f64);
            let limit = v.pointer("/data/limit").and_then(Value::as_f64);
            Ok(match (usage, limit) {
                (Some(u), Some(l)) => format!("The key works. Used ${u:.2} of its ${l:.2} limit."),
                (Some(u), None) => format!("The key works. Used ${u:.2} so far, with no limit set."),
                _ => "The key works.".into(),
            })
        }
        "gemini" => {
            let key = config.gemini_key.as_deref().ok_or("No Gemini key.")?;
            let res = http
                .get(format!("{}/models?pageSize=1", config.gemini_base.trim_end_matches('/')))
                .header("x-goog-api-key", key)
                .send()
                .await
                .map_err(|e| format!("Could not reach Gemini: {}", e.without_url()))?;
            let status = res.status();
            let text = res.text().await.unwrap_or_default();
            if !status.is_success() {
                return Err(provider_error(status, &text));
            }
            Ok("The key works.".into())
        }
        _ => Err(format!("Unknown AI provider \"{provider}\".")),
    }
}
