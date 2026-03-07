#!/bin/bash
set -e

echo "Building frontend..."
cd ui
npm run build
cd ..

echo "Building Rust binary..."
cargo build --release

echo "Done! Binary at ./target/release/stomp_claw"
