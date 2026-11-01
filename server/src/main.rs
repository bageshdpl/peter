use std::sync::Arc;

use peter_server::state::{AppState, BoxError, Config, ConsoleMailer};
use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> Result<(), BoxError> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "peter_server=info,tower_http=info".into()))
        .init();

    let config = Config::from_env();
    let bind = config.bind.clone();
    let state = AppState::connect(config, Arc::new(ConsoleMailer)).await?;

    let listener = tokio::net::TcpListener::bind(&bind).await?;
    tracing::info!("Peter API listening on http://{bind}");
    axum::serve(listener, peter_server::app(state))
        .with_graceful_shutdown(async {
            let _ = tokio::signal::ctrl_c().await;
        })
        .await?;
    Ok(())
}
