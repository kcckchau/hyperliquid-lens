#!/bin/bash
# Removes the hyperliquid-lens launchd service.

set -uo pipefail

PLIST_LABEL="com.hyperliquid-lens"
PLIST_PATH="$HOME/Library/LaunchAgents/${PLIST_LABEL}.plist"

echo "==> Uninstalling hyperliquid-lens daemon..."

launchctl unload "$PLIST_PATH" 2>/dev/null || true
rm -f "$PLIST_PATH"
echo "    Service removed."

echo "==> To re-enable system sleep, run manually:"
echo "    sudo pmset -a sleep 1"
