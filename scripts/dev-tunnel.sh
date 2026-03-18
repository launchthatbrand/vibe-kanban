#!/usr/bin/env bash

set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT_DIR"

if ! command -v cloudflared >/dev/null 2>&1; then
  echo "cloudflared is required. Install it first (e.g. brew install cloudflared)." >&2
  exit 1
fi

FRONTEND_PORT="${FRONTEND_PORT:-$(node scripts/setup-dev-environment.js frontend)}"
BACKEND_PORT="${BACKEND_PORT:-$(node scripts/setup-dev-environment.js backend)}"
PREVIEW_PROXY_PORT="${PREVIEW_PROXY_PORT:-$(node scripts/setup-dev-environment.js preview_proxy)}"

export FRONTEND_PORT
export BACKEND_PORT
export PREVIEW_PROXY_PORT
export VK_SHARED_API_BASE="${VK_SHARED_API_BASE:-http://localhost:3000}"
export VITE_VK_SHARED_API_BASE="${VITE_VK_SHARED_API_BASE:-$VK_SHARED_API_BASE}"
if [[ "${VK_TUNNEL_BYPASS_AUTH:-false}" == "true" ]]; then
  export VITE_BYPASS_AUTH="true"
else
  export VITE_BYPASS_AUTH="false"
fi

TUNNEL_LOG="$(mktemp -t vibe-kanban-cloudflared.XXXXXX.log)"
TUNNEL_PID=""

cleanup() {
  if [[ -n "$TUNNEL_PID" ]] && kill -0 "$TUNNEL_PID" >/dev/null 2>&1; then
    kill "$TUNNEL_PID" >/dev/null 2>&1 || true
  fi
  rm -f "$TUNNEL_LOG"
}

trap cleanup EXIT INT TERM

for port in "$FRONTEND_PORT" "$BACKEND_PORT" "$PREVIEW_PROXY_PORT"; do
  PORT_PIDS="$(lsof -ti tcp:"${port}" -sTCP:LISTEN || true)"
  if [[ -n "$PORT_PIDS" ]]; then
    echo "Stopping existing listener(s) on port ${port}: ${PORT_PIDS}" >&2
    kill $PORT_PIDS >/dev/null 2>&1 || true
  fi
done

TUNNEL_URL=""
for attempt in $(seq 1 3); do
  : >"$TUNNEL_LOG"
  cloudflared tunnel --url "http://localhost:${FRONTEND_PORT}" >"$TUNNEL_LOG" 2>&1 &
  TUNNEL_PID="$!"

  for _ in $(seq 1 60); do
    if ! kill -0 "$TUNNEL_PID" >/dev/null 2>&1; then
      break
    fi

    TUNNEL_URL="$(
      awk 'match($0, /https:\/\/[a-z0-9-]+\.trycloudflare\.com/) { print substr($0, RSTART, RLENGTH); exit }' "$TUNNEL_LOG"
    )"
    if [[ -n "$TUNNEL_URL" ]]; then
      break 2
    fi
    sleep 1
  done

  if kill -0 "$TUNNEL_PID" >/dev/null 2>&1; then
    kill "$TUNNEL_PID" >/dev/null 2>&1 || true
  fi
  TUNNEL_PID=""

  echo "cloudflared attempt ${attempt}/3 failed, retrying..." >&2
done

if [[ -z "$TUNNEL_URL" ]]; then
  echo "Timed out waiting for cloudflared quick tunnel URL." >&2
  cat "$TUNNEL_LOG" >&2
  exit 1
fi

VK_ALLOWED_ORIGINS="http://localhost:${FRONTEND_PORT},${TUNNEL_URL}"
if [[ -n "${VK_EXTRA_ALLOWED_ORIGINS:-}" ]]; then
  VK_ALLOWED_ORIGINS="${VK_ALLOWED_ORIGINS},${VK_EXTRA_ALLOWED_ORIGINS}"
fi
export VK_ALLOWED_ORIGINS

echo ""
echo "Tunnel URL: ${TUNNEL_URL}"
echo "FRONTEND_PORT=${FRONTEND_PORT} BACKEND_PORT=${BACKEND_PORT} PREVIEW_PROXY_PORT=${PREVIEW_PROXY_PORT}"
echo "VK_ALLOWED_ORIGINS=${VK_ALLOWED_ORIGINS}"
echo "VITE_BYPASS_AUTH=${VITE_BYPASS_AUTH}"
echo ""

pnpm exec concurrently "pnpm run backend:dev:watch" "pnpm run local-web:dev"
