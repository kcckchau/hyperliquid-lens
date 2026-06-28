#!/bin/bash
# Startup script run by launchd.
# Waits for Docker Desktop, then runs the full stack in the foreground.
# launchd KeepAlive will restart this script if it exits.

# launchd has a minimal PATH — extend it to cover common Docker locations.
export PATH="/opt/homebrew/bin:/opt/homebrew/sbin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin:$PATH"

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

log() {
  echo "[$(date -u +%Y-%m-%dT%H:%M:%SZ)] $*"
}

log "hyperliquid-lens daemon starting..."
log "Project: $PROJECT_DIR"

# Wait for Docker Desktop to be ready (can take a while after reboot)
until docker info > /dev/null 2>&1; do
  log "Waiting for Docker Desktop..."
  sleep 5
done
log "Docker ready."

cd "$PROJECT_DIR"

# Pull any updated images if online (non-fatal)
# docker compose pull 2>/dev/null || true

log "Starting stack..."
exec docker compose up
