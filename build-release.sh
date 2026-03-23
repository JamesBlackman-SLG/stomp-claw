#!/bin/bash
set -e

echo "Building frontend..."
cd ui
npm run build
cd ..

echo "Building Rust binary..."
cargo build --release

echo "Build complete. Restarting..."
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
"$SCRIPT_DIR/start.sh"
