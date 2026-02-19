//! Tools for discovering and selecting models.
//!
//! Provides utilities to list available models from an upstream provider,
//! as well as the known Claude models that Claude Code expects.

use crate::config::ProxyConfig;
use crate::error::{ProxyError, Result};
use serde::Deserialize;
use std::collections::HashMap;

/// An object representing an OpenAI-compatible model from a `/models` endpoint.
#[derive(Debug, Deserialize)]
pub struct ProviderModel {
    pub id: String,
    pub object: Option<String>,
    pub owned_by: Option<String>,
}

/// The response from an OpenAI-compatible `/models` endpoint.
#[derive(Debug, Deserialize)]
pub struct ProviderModelsResponse {
    pub data: Vec<ProviderModel>,
    pub object: Option<String>,
}

/// An object representing an Anthropic model from `/v1/models`.
#[derive(Debug, Deserialize)]
pub struct AnthropicModel {
    pub id: String,
    #[serde(rename = "type")]
    pub model_type: Option<String>,
    pub display_name: Option<String>,
}

/// The response from an Anthropic `/v1/models` endpoint.
#[derive(Debug, Deserialize)]
pub struct AnthropicModelsResponse {
    pub data: Vec<AnthropicModel>,
}

/// Fetch the list of available models from the configured upstream provider.
///
/// # Errors
/// Returns `ProxyError::Provider` if the request fails or the response cannot be parsed.
pub async fn fetch_provider_models(
    config: &ProxyConfig,
    client: &reqwest::Client,
) -> Result<Vec<String>> {
    let api_key = config.resolve_api_key()?;
    let base_url = config.effective_base_url()?;

    if config.is_anthropic_format() {
        let url = format!("{}/v1/models", base_url.trim_end_matches('/'));
        let response = client
            .get(&url)
            .header("x-api-key", api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await
            .map_err(|e| ProxyError::provider(format!("Failed to fetch Anthropic models: {e}")))?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProxyError::provider(format!(
                "Anthropic API returned status {status} when fetching models: {body}"
            )));
        }

        let parsed: AnthropicModelsResponse = response.json().await.map_err(|e| {
            ProxyError::provider(format!("Failed to parse Anthropic models response: {e}"))
        })?;

        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    } else {
        let url = format!("{}/models", base_url.trim_end_matches('/'));
        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {api_key}"))
            .send()
            .await
            .map_err(|e| ProxyError::provider(format!("Failed to fetch models: {e}")))?;

        let status = response.status().as_u16();
        if status >= 400 {
            let body = response.text().await.unwrap_or_default();
            return Err(ProxyError::provider(format!(
                "Provider returned status {status} when fetching models: {body}"
            )));
        }

        let parsed: ProviderModelsResponse = response
            .json()
            .await
            .map_err(|e| ProxyError::provider(format!("Failed to parse models response: {e}")))?;

        Ok(parsed.data.into_iter().map(|m| m.id).collect())
    }
}

/// Fetch the list of up-to-date models directly from Anthropic's API.
/// This allows an application to dynamically discover what Claude Code expects.
///
/// # Errors
/// Returns `ProxyError::Provider` if the request fails or the response cannot be parsed.
pub async fn fetch_anthropic_models(
    client: &reqwest::Client,
    api_key: &str,
) -> Result<Vec<String>> {
    let url = "https://api.anthropic.com/v1/models";
    let response = client
        .get(url)
        .header("x-api-key", api_key)
        .header("anthropic-version", "2023-06-01")
        .send()
        .await
        .map_err(|e| ProxyError::provider(format!("Failed to fetch Anthropic models: {e}")))?;

    let status = response.status().as_u16();
    if status >= 400 {
        let body = response.text().await.unwrap_or_default();
        return Err(ProxyError::provider(format!(
            "Anthropic API returned status {status} when fetching models: {body}"
        )));
    }

    let parsed: AnthropicModelsResponse = response.json().await.map_err(|e| {
        ProxyError::provider(format!("Failed to parse Anthropic models response: {e}"))
    })?;

    Ok(parsed.data.into_iter().map(|m| m.id).collect())
}

/// Get a fallback list of up-to-date Claude models that Claude Code typically expects or supports.
/// Use this if you don't have an Anthropic API key to call `fetch_anthropic_models`.
#[must_use]
pub fn known_claude_models() -> Vec<&'static str> {
    vec![
        "claude-3-7-sonnet-20250219",
        "claude-3-5-sonnet-20241022",
        "claude-3-5-sonnet-20240620",
        "claude-3-5-haiku-20241022",
        "claude-3-opus-20240229",
        "claude-3-sonnet-20240229",
        "claude-3-haiku-20240307",
    ]
}

/// Returns a best-practice default mapping for the primary Claude models.
///
/// The `default_backend_model` will be used as the target for `claude-3-5-sonnet-20241022`
/// and other primary Claude models if no explicit mapping exists.
#[must_use]
pub fn default_model_mapping(default_backend_model: &str) -> HashMap<String, String> {
    let mut map = HashMap::new();
    for &model in &known_claude_models() {
        map.insert(model.to_string(), default_backend_model.to_string());
    }
    map
}
