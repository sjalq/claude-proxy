use claude_proxy::{build_router, AppState, ProxyConfig, SharedLogger};
use clap::Parser;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[derive(Parser)]
#[command(
    name = "claude-proxy",
    about = "Universal API proxy for Claude Code — route through any LLM provider",
    version
)]
struct Cli {
    /// Path to config file (TOML)
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Port to listen on (overrides config)
    #[arg(short, long)]
    port: Option<u16>,

    /// Provider name (overrides config)
    #[arg(long)]
    provider: Option<String>,

    /// Log file path
    #[arg(long, default_value = "claude-proxy.log")]
    log_file: PathBuf,

    /// Print config search paths and exit
    #[arg(long)]
    show_config_paths: bool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "claude_proxy=info,tower_http=info".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    if cli.show_config_paths {
        println!("Config search paths:");
        println!("  1. claude-proxy.toml (current directory)");
        if cfg!(target_os = "macos") {
            println!("  2. ~/Library/Application Support/claude-proxy/config.toml");
        } else {
            println!("  2. $XDG_CONFIG_HOME/claude-proxy/config.toml");
            println!("     ~/.config/claude-proxy/config.toml");
        }
        println!("  3. ~/.claude-proxy.toml");
        return Ok(());
    }

    let mut config = ProxyConfig::find_and_load(cli.config.as_deref())?;

    if let Some(port) = cli.port {
        config.port = port;
    }
    if let Some(ref provider) = cli.provider {
        config.provider.name = provider.clone();
        if let Some(preset) = claude_proxy::providers::ProviderPreset::from_name(provider) {
            if config.provider.base_url.is_none() {
                config.provider.base_url = Some(preset.base_url.to_string());
            }
            config.provider.api_key_env = preset.default_api_key_env.to_string();
        }
    }

    let logger = SharedLogger::new(&cli.log_file)?;

    // Validate config eagerly
    let base_url = config.effective_base_url()?;
    let _api_key = config.resolve_api_key()?;

    info!("╔═══════════════════════════════════════════════════════╗");
    info!("║           claude-proxy v{}                  ║", env!("CARGO_PKG_VERSION"));
    info!("╚═══════════════════════════════════════════════════════╝");
    info!("  Provider:  {}", config.provider.name);
    info!("  Base URL:  {}", base_url);
    info!("  Format:    {}", if config.is_anthropic_format() { "anthropic (passthrough)" } else { "openai (translate)" });
    info!("  Port:      {}", config.port);
    info!("  Models:    {} mapped", config.models.len());
    info!("  Log file:  {}", cli.log_file.display());

    logger.info("startup", format!(
        "Starting claude-proxy provider={} base_url={} port={}",
        config.provider.name, base_url, config.port
    ));

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(300))
        .build()?;

    let state = Arc::new(AppState {
        config: config.clone(),
        client,
        logger: logger.clone(),
    });

    let app = build_router(state);
    let bind_addr = format!("0.0.0.0:{}", config.port);
    let listener = tokio::net::TcpListener::bind(&bind_addr).await?;

    info!("Listening on http://{}", bind_addr);
    info!("");
    info!("  To use with Claude Code:");
    info!("    ANTHROPIC_BASE_URL=http://localhost:{} claude", config.port);
    info!("");

    axum::serve(listener, app).await?;

    Ok(())
}
