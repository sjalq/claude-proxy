use crate::error::{ProxyError, Result};
use crate::providers::ProviderPreset;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_port")]
    pub port: u16,
    pub provider: ProviderConfig,
    #[serde(default)]
    pub models: HashMap<String, String>,
    #[serde(default)]
    pub params: ParamsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_url: Option<String>,
    #[serde(default = "default_api_key_env")]
    pub api_key_env: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ParamsConfig {
    #[serde(default = "default_drop_params")]
    pub drop: Vec<String>,
}

fn default_port() -> u16 {
    4222
}

fn default_api_key_env() -> String {
    "API_KEY".to_string()
}

fn default_drop_params() -> Vec<String> {
    vec![
        "betas".to_string(),
        "anthropic_beta".to_string(),
        "anthropic-beta".to_string(),
        "context_management".to_string(),
        "reasoning_effort".to_string(),
    ]
}

impl ProxyConfig {
    /// Load config from a TOML file, falling back to defaults.
    pub fn load(path: &Path) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| {
            ProxyError::config(format!("Failed to read config file {}: {}", path.display(), e))
        })?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Search standard locations for a config file.
    /// Priority: CLI arg > CWD > XDG config > home dir
    pub fn find_and_load(explicit_path: Option<&Path>) -> Result<Self> {
        if let Some(path) = explicit_path {
            return Self::load(path);
        }

        let candidates = config_search_paths();
        for candidate in &candidates {
            if candidate.exists() {
                tracing::info!(path = %candidate.display(), "Loading config");
                return Self::load(candidate);
            }
        }

        Err(ProxyError::config(format!(
            "No config file found. Searched: {}. Create one from config.example.toml",
            candidates
                .iter()
                .map(|p| p.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        )))
    }

    /// Resolve the effective base URL (config override or provider preset default)
    pub fn effective_base_url(&self) -> Result<String> {
        if let Some(ref url) = self.provider.base_url {
            return Ok(url.clone());
        }

        let preset = ProviderPreset::from_name(&self.provider.name)
            .ok_or_else(|| {
                ProxyError::config(format!(
                    "Unknown provider '{}' and no base_url configured. \
                     Known providers: openai, openrouter, fireworks, grok, together, groq, anthropic",
                    self.provider.name
                ))
            })?;

        Ok(preset.base_url.to_string())
    }

    /// Resolve the API key from the configured environment variable
    pub fn resolve_api_key(&self) -> Result<String> {
        std::env::var(&self.provider.api_key_env).map_err(|_| {
            ProxyError::config(format!(
                "Environment variable '{}' not set. Set it with your provider API key.",
                self.provider.api_key_env
            ))
        })
    }

    /// Whether this provider uses the Anthropic format (passthrough) vs OpenAI format
    pub fn is_anthropic_format(&self) -> bool {
        if let Some(ref fmt) = self.provider.format {
            return fmt == "anthropic";
        }

        ProviderPreset::from_name(&self.provider.name)
            .map(|p| p.format == "anthropic")
            .unwrap_or(false)
    }
}

fn config_search_paths() -> Vec<PathBuf> {
    let mut paths = Vec::new();

    // CWD
    paths.push(PathBuf::from("claude-proxy.toml"));

    // XDG / platform config dir
    if cfg!(target_os = "macos") {
        if let Some(home) = dirs_path() {
            paths.push(
                home.join("Library")
                    .join("Application Support")
                    .join("claude-proxy")
                    .join("config.toml"),
            );
        }
    } else {
        if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
            paths.push(
                PathBuf::from(xdg)
                    .join("claude-proxy")
                    .join("config.toml"),
            );
        }
        if let Some(home) = dirs_path() {
            paths.push(
                home.join(".config")
                    .join("claude-proxy")
                    .join("config.toml"),
            );
        }
    }

    // Home directory fallback
    if let Some(home) = dirs_path() {
        paths.push(home.join(".claude-proxy.toml"));
    }

    paths
}

fn dirs_path() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_config() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            r#"
port = 5000

[provider]
name = "openai"
api_key_env = "OPENAI_API_KEY"

[models]
"claude-sonnet-4-20250514" = "gpt-4o"

[params]
drop = ["betas"]
"#
        )
        .unwrap();

        let config = ProxyConfig::load(f.path()).unwrap();
        assert_eq!(config.port, 5000);
        assert_eq!(config.provider.name, "openai");
        assert_eq!(
            config.models.get("claude-sonnet-4-20250514"),
            Some(&"gpt-4o".to_string())
        );
    }

    #[test]
    fn test_effective_base_url_from_preset() {
        let config = ProxyConfig {
            port: 4222,
            provider: ProviderConfig {
                name: "openai".to_string(),
                base_url: None,
                api_key_env: "OPENAI_API_KEY".to_string(),
                format: None,
            },
            models: HashMap::new(),
            params: ParamsConfig::default(),
        };

        let url = config.effective_base_url().unwrap();
        assert_eq!(url, "https://api.openai.com/v1");
    }

    #[test]
    fn test_effective_base_url_override() {
        let config = ProxyConfig {
            port: 4222,
            provider: ProviderConfig {
                name: "custom".to_string(),
                base_url: Some("https://my-server.com/v1".to_string()),
                api_key_env: "MY_KEY".to_string(),
                format: None,
            },
            models: HashMap::new(),
            params: ParamsConfig::default(),
        };

        let url = config.effective_base_url().unwrap();
        assert_eq!(url, "https://my-server.com/v1");
    }
}
