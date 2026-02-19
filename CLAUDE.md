# Claude Proxy

Universal API proxy for Claude Code — route through any LLM provider.

## Architecture

```
Claude Code CLI → ANTHROPIC_BASE_URL=localhost:4222 → claude-proxy → Any Provider
```

The proxy accepts Anthropic Messages API requests and translates them to OpenAI Chat Completions format (or passes through for Anthropic direct).

## Building

```bash
cargo build          # debug build
cargo test           # unit + integration tests (needs FIREWORKS_API_KEY for integration)
```

## Running

```bash
# 1. Create config from example
cp config.example.toml claude-proxy.toml
# Edit claude-proxy.toml with your provider and model mappings

# 2. Set API key
export FIREWORKS_API_KEY=fw_xxx

# 3. Run proxy
cargo run

# 4. Use with Claude Code
ANTHROPIC_BASE_URL=http://localhost:4222 claude
```

## Code Style

- Pure functional where possible: translation functions take inputs, return outputs
- All errors via thiserror `ProxyError` enum with helper constructors
- Comprehensive structured logging via `SharedLogger` (JSONL ring buffer)
- Streaming uses a state machine (`StreamTranslator`) — no hidden mutation outside the translator
- Library + binary split: `src/lib.rs` exports everything for integration into other Rust projects

## Key Modules

| Module | Purpose |
|--------|---------|
| `translate/anthropic_types` | Anthropic Messages API types |
| `translate/openai_types` | OpenAI Chat Completions types |
| `translate/request` | Anthropic → OpenAI request translation |
| `translate/response` | OpenAI → Anthropic response translation |
| `translate/streaming` | SSE stream chunk translation state machine |
| `config` | TOML config + env var loading |
| `providers` | Built-in provider presets |
| `proxy` | Core forwarding (streaming + non-streaming) |
| `server` | Axum HTTP server + routes |
| `logging` | JSONL ring-buffer logger |
