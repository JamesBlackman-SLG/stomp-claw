#!/bin/bash
pkill -f stomp-claw-viewer 2>/dev/null || true
sleep 0.5
cargo run --release --bin stomp-claw-viewer
