#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Kill existing stomp_claw (and old separate viewer if running)
pkill -f "stomp_claw" 2>/dev/null || true
pkill -f "stomp-claw-viewer" 2>/dev/null || true
sleep 1

# Start stomp_claw from this directory (so it finds the beep WAVs)
# Redirect output and disown so parent shells don't hang waiting on the pipe
echo "Starting stomp_claw..."
set -a; source "$SCRIPT_DIR/.env" 2>/dev/null; set +a
nohup ./target/release/stomp_claw >> ~/.stomp-claw/stomp-claw.log 2>&1 &
disown

sleep 2
echo "stomp_claw started. Log at ~/.stomp-claw/stomp-claw.log"
tail -5 ~/.stomp-claw/stomp-claw.log
