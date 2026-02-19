# claude-proxy

[![CI](https://github.com/sjalq/claude-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/sjalq/claude-proxy/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](LICENSE)

**Universal API proxy for Claude Code** — route through any LLM provider.

A single Rust binary that sits between [Claude Code](https://github.com/anthropics/claude-code) and any OpenAI-compatible API provider. It translates Anthropic Messages API requests into OpenAI Chat Completions format on the fly, including full streaming SSE and tool use.

```
Claude Code CLI  →  claude-proxy (localhost:4222)  →  Any Provider
                    Anthropic format in               OpenAI format out
```

## Why?

Claude Code only speaks the Anthropic Messages API. This proxy lets you point it at:

| Provider | Status | Models |
|----------|--------|--------|
| **Fireworks** | Tested | Kimi K2.5, K2-Instruct |
| **OpenAI** | Supported | GPT-4o, o1, o3, etc. |
| **OpenRouter** | Supported | Any model on OpenRouter |
| **Grok (xAI)** | Supported | Grok-3, Grok-3-mini |
| **Together** | Supported | Llama, Mixtral, etc. |
| **Groq** | Supported | Llama, Mixtral (fast) |
| **DeepSeek** | Supported | DeepSeek-R1, V3 |
| **Anthropic** | Passthrough | Claude (direct, no translation) |
| **Custom** | Supported | Any OpenAI-compatible endpoint |

## Quick Start

### From source

```bash
git clone https://github.com/sjalq/claude-proxy.git
cd claude-proxy
cargo build --release
```

### Configure

```bash
cp config.example.toml claude-proxy.toml
```

Edit `claude-proxy.toml` with your provider and model mappings. Example for Fireworks:

```toml
port = 4222

[provider]
name = "fireworks"
api_key_env = "FIREWORKS_API_KEY"

[models]
"claude-sonnet-4-20250514" = "accounts/fireworks/models/kimi-k2p5"
"claude-opus-4-20250514" = "accounts/fireworks/models/kimi-k2p5"
"claude-haiku-4-5-20251001" = "accounts/fireworks/models/kimi-k2-instruct-0905"
```

### Run

```bash
export FIREWORKS_API_KEY=fw_your_key_here
./target/release/claude-proxy
```

### Use with Claude Code

```bash
ANTHROPIC_BASE_URL=http://localhost:4222 claude
```

## Provider Setup

<details>
<summary><strong>OpenAI</strong></summary>

```toml
[provider]
name = "openai"
api_key_env = "OPENAI_API_KEY"

[models]
"claude-sonnet-4-20250514" = "gpt-4o"
"claude-opus-4-20250514" = "gpt-4o"
"claude-haiku-4-5-20251001" = "gpt-4o-mini"
```
</details>

<details>
<summary><strong>OpenRouter</strong></summary>

```toml
[provider]
name = "openrouter"
api_key_env = "OPENROUTER_API_KEY"

[models]
"claude-sonnet-4-20250514" = "anthropic/claude-sonnet-4"
"claude-opus-4-20250514" = "openai/gpt-4o"
"claude-haiku-4-5-20251001" = "google/gemini-2.0-flash"
```
</details>

<details>
<summary><strong>Grok (xAI)</strong></summary>

```toml
[provider]
name = "grok"
api_key_env = "XAI_API_KEY"

[models]
"claude-sonnet-4-20250514" = "grok-3"
"claude-opus-4-20250514" = "grok-3"
"claude-haiku-4-5-20251001" = "grok-3-mini"
```
</details>

<details>
<summary><strong>Fireworks (Kimi K2.5)</strong></summary>

```toml
[provider]
name = "fireworks"
api_key_env = "FIREWORKS_API_KEY"

[models]
"claude-sonnet-4-20250514" = "accounts/fireworks/models/kimi-k2p5"
"claude-opus-4-20250514" = "accounts/fireworks/models/kimi-k2p5"
"claude-haiku-4-5-20251001" = "accounts/fireworks/models/kimi-k2-instruct-0905"
```
</details>

<details>
<summary><strong>Together</strong></summary>

```toml
[provider]
name = "together"
api_key_env = "TOGETHER_API_KEY"

[models]
"claude-sonnet-4-20250514" = "meta-llama/Meta-Llama-3.1-405B-Instruct-Turbo"
"claude-haiku-4-5-20251001" = "meta-llama/Meta-Llama-3.1-70B-Instruct-Turbo"
```
</details>

<details>
<summary><strong>Custom Provider</strong></summary>

Any OpenAI-compatible endpoint works:

```toml
[provider]
name = "custom"
base_url = "https://your-server.com/v1"
api_key_env = "YOUR_API_KEY"
format = "openai"

[models]
"claude-sonnet-4-20250514" = "your-model-name"
```
</details>

## Configuration Reference

```toml
# Port the proxy listens on
port = 4222

[provider]
name = "fireworks"                          # Provider preset or "custom"
# base_url = "https://..."                  # Override (presets have defaults)
api_key_env = "FIREWORKS_API_KEY"           # Env var holding the API key
# format = "openai"                         # "openai" (translate) or "anthropic" (passthrough)

[models]
# Map Claude model names → provider model names
# Unmapped models pass through as-is
"claude-sonnet-4-20250514" = "accounts/fireworks/models/kimi-k2p5"

[params]
# Anthropic-specific params to drop when forwarding
drop = ["betas", "anthropic_beta", "context_management", "reasoning_effort"]
```

## CLI Options

```
claude-proxy [OPTIONS]

Options:
  -c, --config <PATH>      Path to config file (TOML)
  -p, --port <PORT>        Port to listen on (overrides config)
      --provider <NAME>    Provider name (overrides config)
      --log-file <PATH>    Log file path [default: claude-proxy.log]
      --show-config-paths  Print config search paths and exit
  -h, --help               Print help
  -V, --version            Print version
```

Config file search order:
1. `--config <path>` (explicit)
2. `./claude-proxy.toml` (current directory)
3. `~/Library/Application Support/claude-proxy/config.toml` (macOS)
   `~/.config/claude-proxy/config.toml` (Linux)
4. `~/.claude-proxy.toml`

## Library Usage

`claude-proxy` is also a Rust library. Add it to your `Cargo.toml`:

```toml
[dependencies]
claude-proxy = { git = "https://github.com/sjalq/claude-proxy" }
```

### Translation only (no server)

```rust
use claude_proxy::translate::request::anthropic_to_openai;
use claude_proxy::translate::response::openai_to_anthropic;
use claude_proxy::translate::streaming::StreamTranslator;
use std::collections::HashMap;

// Translate a request
let model_map = HashMap::from([
    ("claude-sonnet-4-20250514".into(), "gpt-4o".into()),
]);
let openai_req = anthropic_to_openai(&anthropic_req, &model_map);

// Translate streaming chunks
let mut translator = StreamTranslator::new("claude-sonnet-4-20250514");
for chunk in openai_chunks {
    let events = translator.process_chunk(&chunk);
    // Each event is an Anthropic SSE event ready to send
}
let final_events = translator.finish();
```

### Embed the proxy server

```rust
use claude_proxy::{build_router, AppState, ProxyConfig, SharedLogger};
use std::sync::Arc;

let config = ProxyConfig::find_and_load(None)?;
let logger = SharedLogger::new("proxy.log")?;
let client = reqwest::Client::new();

let state = Arc::new(AppState { config, client, logger });
let app = build_router(state);

let listener = tokio::net::TcpListener::bind("0.0.0.0:4222").await?;
axum::serve(listener, app).await?;
```

## How Translation Works

### Request (Anthropic → OpenAI)

| Anthropic | OpenAI |
|-----------|--------|
| `system` field | `{"role": "system"}` message |
| `messages[].content` (text) | `messages[].content` (text) |
| `messages[].content` (image base64) | `image_url` with data URI |
| `tools[].input_schema` | `tools[].function.parameters` |
| `tool_use` content block | `tool_calls[]` on message |
| `tool_result` content block | `{"role": "tool"}` message |
| `tool_choice: "any"` | `tool_choice: "required"` |

### Response (OpenAI → Anthropic)

| OpenAI | Anthropic |
|--------|-----------|
| `choices[0].message.content` | `content[{type: "text"}]` |
| `choices[0].message.tool_calls` | `content[{type: "tool_use"}]` |
| `finish_reason: "stop"` | `stop_reason: "end_turn"` |
| `finish_reason: "tool_calls"` | `stop_reason: "tool_use"` |
| `finish_reason: "length"` | `stop_reason: "max_tokens"` |
| `usage.prompt_tokens` | `usage.input_tokens` |
| `delta.reasoning_content` | `content_block_delta` (text) |

### Streaming SSE

OpenAI streams `data: {chunk}` lines. The proxy translates these into Anthropic's named SSE events:

```
message_start → content_block_start → content_block_delta* → content_block_stop → message_delta → message_stop
```

Reasoning models (Kimi K2.5, DeepSeek R1) that stream chain-of-thought via `reasoning_content` are automatically handled.

## Architecture

```
src/
├── lib.rs                      # Library exports
├── main.rs                     # CLI binary with graceful shutdown
├── config.rs                   # TOML config + env vars
├── error.rs                    # Error types (thiserror)
├── logging.rs                  # JSONL ring-buffer logger
├── providers.rs                # 8 built-in provider presets
├── proxy.rs                    # Forwarding with retry logic
├── server.rs                   # Axum HTTP server
└── translate/
    ├── anthropic_types.rs      # Anthropic Messages API types
    ├── openai_types.rs         # OpenAI Chat Completions types
    ├── request.rs              # Anthropic → OpenAI
    ├── response.rs             # OpenAI → Anthropic
    └── streaming.rs            # SSE state machine
```

## License

MIT
