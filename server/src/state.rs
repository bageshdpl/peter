use std::{str::FromStr, sync::Arc};

use sqlx::{
    SqlitePool,
    sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions},
};

#[derive(Debug, Clone)]
pub struct Config {
    pub database_url: String,
    pub bind: String,
    /// Where the web app runs. Used for CORS and for links in emails.
    pub app_url: String,
    pub access_ttl_secs: i64,
    pub refresh_ttl_secs: i64,
    pub invite_ttl_secs: i64,
    pub reset_ttl_secs: i64,
    pub allow_signup: bool,
    /// With sign-up off, this email may still create the first account on an
    /// empty server (the owner bootstrapping an invite-only deployment).
    pub admin_email: Option<String>,
    /// Where uploaded files are kept.
    pub data_dir: std::path::PathBuf,
    pub max_upload_bytes: usize,
    pub ai: crate::ai::AiConfig,
    /// The built web app to serve alongside the API (the ready-to-run package).
    pub web_dir: Option<std::path::PathBuf>,
    /// The extension's folder, offered as a download set up for this server.
    pub extension_dir: Option<std::path::PathBuf>,
}

impl Config {
    pub fn from_env() -> Self {
        let var = |name: &str, default: &str| std::env::var(name).unwrap_or_else(|_| default.to_string());
        let num = |name: &str, default: i64| {
            std::env::var(name).ok().and_then(|v| v.parse().ok()).unwrap_or(default)
        };
        Self {
            database_url: var("PETER_DATABASE_URL", "sqlite://peter.db"),
            bind: var("PETER_BIND", "127.0.0.1:8080"),
            app_url: var("PETER_APP_URL", "http://localhost:5173"),
            access_ttl_secs: num("PETER_ACCESS_TTL_SECS", 15 * 60),
            refresh_ttl_secs: num("PETER_REFRESH_TTL_SECS", 30 * 24 * 3600),
            invite_ttl_secs: num("PETER_INVITE_TTL_SECS", 7 * 24 * 3600),
            reset_ttl_secs: num("PETER_RESET_TTL_SECS", 3600),
            allow_signup: var("PETER_ALLOW_SIGNUP", "true") != "false",
            admin_email: std::env::var("PETER_ADMIN_EMAIL").ok().map(|e| e.trim().to_lowercase()).filter(|e| !e.is_empty()),
            data_dir: var("PETER_DATA_DIR", "peter-data").into(),
            max_upload_bytes: num("PETER_MAX_UPLOAD_MB", 25) as usize * 1024 * 1024,
            ai: crate::ai::AiConfig::from_env(),
            web_dir: std::env::var("PETER_WEB_DIR").ok().filter(|v| !v.is_empty()).map(Into::into),
            extension_dir: std::env::var("PETER_EXTENSION_DIR").ok().filter(|v| !v.is_empty()).map(Into::into),
        }
    }

    pub fn for_tests() -> Self {
        let data_dir = std::env::temp_dir().join(format!("peter-test-{}", uuid::Uuid::new_v4().simple()));
        // Tests never use real keys; they point providers at local fakes when needed.
        let ai = crate::ai::AiConfig { openrouter_key: None, gemini_key: None, sample: true, ..crate::ai::AiConfig::from_env() };
        Self { database_url: "sqlite::memory:".into(), data_dir, max_upload_bytes: 1024 * 1024, ai, ..Self::from_env() }
    }
}

/// Delivers links that would normally go out by email. Locally they are
/// logged to the console; tests capture them.
pub trait Mailer: Send + Sync {
    fn send_link(&self, to: &str, subject: &str, link: &str);
}

pub struct ConsoleMailer;

impl Mailer for ConsoleMailer {
    fn send_link(&self, to: &str, subject: &str, link: &str) {
        tracing::info!(%to, %subject, %link, "email (console delivery)");
    }
}

/// Something that happened in a validation session, pushed to open pages.
#[derive(Debug, Clone)]
pub struct SessionEvent {
    pub workspace_id: String,
    pub session_id: String,
    pub name: &'static str,
    pub data: serde_json::Value,
}

#[derive(Clone)]
pub struct AppState {
    pub db: SqlitePool,
    pub config: Arc<Config>,
    pub mailer: Arc<dyn Mailer>,
    /// A real Argon2 hash, verified against when an email is unknown so that
    /// sign-in takes the same time whether or not the account exists.
    pub dummy_hash: Arc<String>,
    pub events: tokio::sync::broadcast::Sender<SessionEvent>,
}

impl AppState {
    pub async fn connect(config: Config, mailer: Arc<dyn Mailer>) -> Result<Self, BoxError> {
        let in_memory = config.database_url.contains(":memory:");
        let options = SqliteConnectOptions::from_str(&config.database_url)?
            .create_if_missing(true)
            .foreign_keys(true)
            .journal_mode(if in_memory { SqliteJournalMode::Memory } else { SqliteJournalMode::Wal });
        // An in-memory database exists per connection, so tests use exactly one.
        let db = SqlitePoolOptions::new()
            .max_connections(if in_memory { 1 } else { 8 })
            .connect_with(options)
            .await?;
        sqlx::migrate!("./migrations").run(&db).await?;
        tokio::fs::create_dir_all(config.data_dir.join("sources")).await?;
        let dummy_hash = crate::security::hash_password("peter-dummy-password")
            .map_err(|_| BoxError::from("could not hash the dummy password"))?;
        tokio::fs::create_dir_all(config.data_dir.join("screenshots")).await?;
        // A draft cannot survive a restart; say so rather than leave it spinning.
        sqlx::query("UPDATE documents SET status = 'failed', error = 'Drafting stopped because the server restarted. Draft again.' WHERE status = 'drafting'")
            .execute(&db)
            .await?;
        let (events, _) = tokio::sync::broadcast::channel(512);
        Ok(Self { db, config: Arc::new(config), mailer, dummy_hash: Arc::new(dummy_hash), events })
    }
}

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;
