# Changelog

## [0.1.0] - 2025-02-19

### Added
- Initial release
- Anthropic Messages API to OpenAI Chat Completions translation
- Full streaming SSE translation with state machine
- Tool use / function calling translation (both directions)
- Image content (base64) translation
- Support for reasoning models (Kimi K2.5, DeepSeek R1) via `reasoning_content`
- 8 built-in provider presets: OpenAI, OpenRouter, Fireworks, Grok, Together, Groq, DeepSeek, Anthropic
- Custom provider support via TOML config
- Model name mapping (Claude model names → provider model names)
- Anthropic passthrough mode (no translation for direct Anthropic use)
- Automatic retry with exponential backoff on transient errors (429, 5xx)
- Graceful shutdown on SIGTERM/SIGINT
- TOML configuration with environment variable API key resolution
- JSONL ring-buffer structured logging
- Library + binary split for embedding in other Rust projects
- Comprehensive test suite (unit + integration tests against Fireworks K2.5)
- GitHub Actions CI (fmt, clippy, test on Linux/macOS/Windows)
