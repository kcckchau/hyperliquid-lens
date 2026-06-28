#!/bin/bash
# Installs hyperliquid-lens as a macOS launchd service.
# Run once: bash scripts/install-service.sh
# Requires: Docker Desktop installed, macOS.

set -uo pipefail

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
PLIST_LABEL="com.hyperliquid-lens"
PLIST_PATH="$HOME/Library/LaunchAgents/${PLIST_LABEL}.plist"
LOG_DIR="$HOME/Library/Logs/hyperliquid-lens"

echo "==> Installing hyperliquid-lens daemon"
echo "    Project: $PROJECT_DIR"
echo "    Plist:   $PLIST_PATH"
echo "    Logs:    $LOG_DIR"
echo ""

# Sanity checks
if ! command -v docker &> /dev/null; then
  echo "ERROR: docker not found. Install Docker Desktop first."
  exit 1
fi

if [[ "$(uname)" != "Darwin" ]]; then
  echo "ERROR: This script is macOS only."
  exit 1
fi

# Create log directory
mkdir -p "$LOG_DIR"

# Disable system sleep — safe for Mac Studio (desktop, no battery).
# pmset requires an interactive sudo session so we print the command instead
# of running it here. Run it once manually in your terminal.
echo "==> Sleep prevention (run manually if not already done):"
echo "    sudo pmset -a sleep 0 && sudo pmset -a disksleep 0"
echo "    (or: System Settings → Energy → disable automatic sleep)"
echo ""

# Generate plist with absolute paths resolved at install time
cat > "$PLIST_PATH" << PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>${PLIST_LABEL}</string>

    <key>ProgramArguments</key>
    <array>
        <string>/bin/bash</string>
        <string>${PROJECT_DIR}/scripts/start-daemon.sh</string>
    </array>

    <!-- Start immediately when loaded, and on every login/reboot -->
    <key>RunAtLoad</key>
    <true/>

    <!-- launchd will restart the script if it exits for any reason -->
    <key>KeepAlive</key>
    <true/>

    <!-- Minimum seconds between restarts (prevents tight crash loops) -->
    <key>ThrottleInterval</key>
    <integer>30</integer>

    <key>StandardOutPath</key>
    <string>${LOG_DIR}/daemon.log</string>
    <key>StandardErrorPath</key>
    <string>${LOG_DIR}/daemon-error.log</string>
</dict>
</plist>
PLIST

echo "==> Plist written."

# Unload first if already installed
launchctl unload "$PLIST_PATH" 2>/dev/null || true

# Load the service
launchctl load "$PLIST_PATH"
echo "==> Service loaded."
echo ""
echo "Done. The daemon will now start on every login/reboot."
echo ""
echo "Useful commands:"
echo "  make service-status   # is it running?"
echo "  make service-logs     # tail logs"
echo "  make uninstall-service"
