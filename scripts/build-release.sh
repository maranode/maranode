#!/usr/bin/env bash
# Build a reproducible release binary locally.
# Mirrors the SOURCE_DATE_EPOCH logic used in CI so the resulting checksums
# match the official release artifacts when built from the same commit.
#
# Usage:
#   ./scripts/build-release.sh                  # auto-detect backend
#   ./scripts/build-release.sh --features metal
#   ./scripts/build-release.sh --features cuda
set -euo pipefail

ROOT=$(cd "$(dirname "$0")/.." && pwd)
cd "$ROOT"

FEATURES="${1:-}"

# Reproducible timestamp
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
echo "SOURCE_DATE_EPOCH=${SOURCE_DATE_EPOCH} ($(date -d "@${SOURCE_DATE_EPOCH}" 2>/dev/null || date -r "${SOURCE_DATE_EPOCH}"))"

# Auto-detect backend if no --features flag is provided
if [ -z "$FEATURES" ]; then
  if [ "$(uname)" = "Darwin" ]; then
    FEATURES="--features metal"
    echo "macOS detected - using Metal backend"
  elif command -v nvcc > /dev/null 2>&1 || [ -d /usr/local/cuda ]; then
    FEATURES="--features cuda"
    echo "CUDA detected - using CUDA backend"
  elif command -v hipcc > /dev/null 2>&1 || [ -d /opt/rocm ]; then
    FEATURES="--features rocm"
    echo "ROCm detected - using ROCm backend"
  else
    echo "No GPU SDK - building CPU-only"
  fi
fi

#Build
echo "Building maranoded and maranode..."
cargo build --release \
  --bin maranoded --bin maranode \
  --no-default-features \
  $FEATURES

#Checksum
BIN_DIR="target/release"
echo ""
echo "Checksums:"
sha256sum "${BIN_DIR}/maranoded" "${BIN_DIR}/maranode"

echo ""
echo "Binaries: ${BIN_DIR}/maranoded  ${BIN_DIR}/maranode"
