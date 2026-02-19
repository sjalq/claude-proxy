//! Built-in provider presets for common LLM API providers.
//!
//! Each preset defines the base URL, API format, and default environment variable
//! for the API key. Users specify a provider name in their config and the preset
//! fills in the details.

/// Built-in provider presets. Each preset defines the base URL and API format
/// so users only need to specify a provider name in their config.
#[derive(Debug, Clone)]
pub struct ProviderPreset {
    pub name: &'static str,
    pub base_url: &'static str,
    pub format: &'static str, // "openai" or "anthropic"
    pub default_api_key_env: &'static str,
}

const PRESETS: &[ProviderPreset] = &[
    ProviderPreset {
        name: "openai",
        base_url: "https://api.openai.com/v1",
        format: "openai",
        default_api_key_env: "OPENAI_API_KEY",
    },
    ProviderPreset {
        name: "openrouter",
        base_url: "https://openrouter.ai/api/v1",
        format: "openai",
        default_api_key_env: "OPENROUTER_API_KEY",
    },
    ProviderPreset {
        name: "fireworks",
        base_url: "https://api.fireworks.ai/inference/v1",
        format: "openai",
        default_api_key_env: "FIREWORKS_API_KEY",
    },
    ProviderPreset {
        name: "grok",
        base_url: "https://api.x.ai/v1",
        format: "openai",
        default_api_key_env: "XAI_API_KEY",
    },
    ProviderPreset {
        name: "together",
        base_url: "https://api.together.xyz/v1",
        format: "openai",
        default_api_key_env: "TOGETHER_API_KEY",
    },
    ProviderPreset {
        name: "groq",
        base_url: "https://api.groq.com/openai/v1",
        format: "openai",
        default_api_key_env: "GROQ_API_KEY",
    },
    ProviderPreset {
        name: "anthropic",
        base_url: "https://api.anthropic.com",
        format: "anthropic",
        default_api_key_env: "ANTHROPIC_API_KEY",
    },
    ProviderPreset {
        name: "deepseek",
        base_url: "https://api.deepseek.com/v1",
        format: "openai",
        default_api_key_env: "DEEPSEEK_API_KEY",
    },
];

impl ProviderPreset {
    #[must_use]
    pub fn from_name(name: &str) -> Option<&'static ProviderPreset> {
        PRESETS.iter().find(|p| p.name == name.to_lowercase())
    }

    #[must_use]
    pub fn all() -> &'static [ProviderPreset] {
        PRESETS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_known_providers() {
        assert!(ProviderPreset::from_name("openai").is_some());
        assert!(ProviderPreset::from_name("fireworks").is_some());
        assert!(ProviderPreset::from_name("OpenRouter").is_some()); // case-insensitive
        assert!(ProviderPreset::from_name("unknown_provider").is_none());
    }

    #[test]
    fn test_anthropic_is_anthropic_format() {
        let preset = ProviderPreset::from_name("anthropic").unwrap();
        assert_eq!(preset.format, "anthropic");
    }

    #[test]
    fn test_all_others_are_openai_format() {
        for preset in ProviderPreset::all() {
            if preset.name != "anthropic" {
                assert_eq!(
                    preset.format, "openai",
                    "Provider {} should be openai format",
                    preset.name
                );
            }
        }
    }
}
