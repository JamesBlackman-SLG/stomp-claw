#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# Kill existing stomp_claw (and old separate viewer if running)
pkill -f "stomp_claw" 2>/dev/null || true
pkill -f "stomp-claw-viewer" 2>/dev/null || true
sleep 1

# Start stomp_claw from this directory (so it finds the beep WAVs)
echo "Starting stomp_claw..."
./target/release/stomp_claw &

sleep 2
echo "stomp_claw started. Log at ~/.stomp-claw/stomp-claw.log"
tail -5 ~/.stomp-claw/stomp-claw.log
