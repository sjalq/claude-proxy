//! Error types for the proxy.

use thiserror::Error;

#[derive(Error, Debug)]
#[non_exhaustive]
pub enum ProxyError {
    #[error("Configuration error: {message}")]
    Config { message: String },

    #[error("Provider error: {message}")]
    Provider { message: String },

    #[error("Translation error: {message}")]
    Translation { message: String },

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("TOML parse error: {0}")]
    Toml(#[from] toml::de::Error),

    #[error("{0}")]
    Other(String),
}

impl ProxyError {
    pub fn config(msg: impl Into<String>) -> Self {
        Self::Config {
            message: msg.into(),
        }
    }

    pub fn provider(msg: impl Into<String>) -> Self {
        Self::Provider {
            message: msg.into(),
        }
    }

    pub fn translation(msg: impl Into<String>) -> Self {
        Self::Translation {
            message: msg.into(),
        }
    }

    pub fn other(msg: impl Into<String>) -> Self {
        Self::Other(msg.into())
    }
}

pub type Result<T> = std::result::Result<T, ProxyError>;
