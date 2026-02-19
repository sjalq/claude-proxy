#!/bin/bash
set -euo pipefail

# Thin wrapper: start claude-proxy and launch claude through it.
# Usage: ./scripts/claude-proxy.sh [claude args...]

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
PROXY_PORT="${CLAUDE_PROXY_PORT:-4222}"
PROXY_PID=""

cleanup() {
    if [[ -n "$PROXY_PID" ]] && kill -0 "$PROXY_PID" 2>/dev/null; then
        kill "$PROXY_PID" 2>/dev/null || true
    fi
}
trap cleanup EXIT INT TERM

BINARY="$PROJECT_DIR/target/debug/claude-proxy"
if [[ ! -x "$BINARY" ]]; then
    BINARY="$PROJECT_DIR/target/release/claude-proxy"
fi

if [[ ! -x "$BINARY" ]]; then
    echo "Building claude-proxy..."
    (cd "$PROJECT_DIR" && cargo build)
    BINARY="$PROJECT_DIR/target/debug/claude-proxy"
fi

echo "Starting claude-proxy on port $PROXY_PORT..."
"$BINARY" --port "$PROXY_PORT" &
PROXY_PID=$!

# Wait for proxy to be ready
for i in $(seq 1 15); do
    if curl -s "http://localhost:$PROXY_PORT/health" > /dev/null 2>&1; then
        break
    fi
    sleep 0.5
done

if ! curl -s "http://localhost:$PROXY_PORT/health" > /dev/null 2>&1; then
    echo "Error: Proxy failed to start" >&2
    exit 1
fi

echo "Proxy ready. Launching claude..."
ANTHROPIC_BASE_URL="http://localhost:$PROXY_PORT" \
ANTHROPIC_API_KEY="proxy-key" \
claude "$@"
