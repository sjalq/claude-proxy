//! # claude-proxy
//!
//! Universal API proxy for Claude Code — route through any LLM provider.
//!
//! This crate translates [Anthropic Messages API](https://docs.anthropic.com/en/api/messages)
//! requests into [OpenAI Chat Completions](https://platform.openai.com/docs/api-reference/chat)
//! format and back, enabling Claude Code to work with any OpenAI-compatible provider.
//!
//! ## Usage modes
//!
//! **As a standalone binary:** Run `claude-proxy` to start a local HTTP server, then point
//! Claude Code at it via `ANTHROPIC_BASE_URL`.
//!
//! **As a library:** Use the [`translate`] module for request/response translation,
//! or embed the full proxy server with [`build_router`].
//!
//! ## Quick example
//!
//! ```rust,no_run
//! use claude_proxy::{build_router, AppState, ProxyConfig, SharedLogger};
//! use std::sync::Arc;
//!
//! # async fn run() -> anyhow::Result<()> {
//! let config = ProxyConfig::find_and_load(None)?;
//! let logger = SharedLogger::new("proxy.log")?;
//! let client = reqwest::Client::new();
//!
//! let state = Arc::new(AppState { config, client, logger });
//! let app = build_router(state);
//!
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:4222").await?;
//! axum::serve(listener, app).await?;
//! # Ok(())
//! # }
//! ```

pub mod config;
pub mod error;
pub mod logging;
pub mod providers;
pub mod proxy;
pub mod server;
pub mod translate;

pub use config::ProxyConfig;
pub use error::{ProxyError, Result};
pub use logging::SharedLogger;
pub use server::{build_router, AppState};
