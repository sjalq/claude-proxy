//! Start a claude-proxy server programmatically.
//!
//! Usage:
//!   export FIREWORKS_API_KEY=fw_your_key
//!   cargo run --example basic_proxy

use claude_proxy::{build_router, AppState, ProxyConfig, SharedLogger};
use std::sync::Arc;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt::init();

    let config = ProxyConfig::find_and_load(None)?;
    let base_url = config.effective_base_url()?;

    println!("Provider: {} ({})", config.provider.name, base_url);
    println!("Models mapped: {}", config.models.len());

    let logger = SharedLogger::new("proxy-example.log")?;
    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let port = config.port;
    let state = Arc::new(AppState {
        config,
        client,
        logger,
    });

    let app = build_router(state);
    let addr = format!("0.0.0.0:{}", port);
    let listener = tokio::net::TcpListener::bind(&addr).await?;

    println!("Listening on http://{}", addr);
    println!();
    println!("  ANTHROPIC_BASE_URL=http://localhost:{} claude", port);

    axum::serve(listener, app).await?;
    Ok(())
}
