#!/bin/bash
# Build and restart jcode in one shot.
# Usage: scripts/rebuild.sh

set -euo pipefail

cd "$(dirname "$0")/.."

echo "==> Building jcode..."
cargo build --release 2>&1 | tail -3

echo "==> Installing..."
cp target/release/jcode ~/.jcode/builds/current/jcode
strip ~/.jcode/builds/current/jcode

echo "==> Killing old server daemon..."
pkill -f "jcode.*serve" 2>/dev/null || true
sleep 0.5

echo "==> Done. Binary: $(ls -lh ~/.jcode/builds/current/jcode | awk '{print $5}')"
echo "    Server stopped. Launch jcode again to use the new binary."
echo "    (Cmd+; or ~/.jcode/builds/current/jcode)"
