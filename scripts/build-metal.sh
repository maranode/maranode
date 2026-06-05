#!/usr/bin/env bash
# build maranoded with Apple Metal GPU acceleration.
#
# Requirements:
#   - macOS 13 Ventura or later (Apple Silicon or Intel with AMD/Intel GPU)
#   - Xcode Command Line Tools (clang, Metal framework headers)
#   - cmake 3.14+ (brew install cmake)
#   - Rust 1.86+
#
# usage:
#   ./scripts/build-metal.sh            # debug build
#   ./scripts/build-metal.sh --release  # release build
#   ./scripts/build-metal.sh --check    # type-check only (fast)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

MODE="debug"
CARGO_CMD="build"

for arg in "$@"; do
    case "$arg" in
        --release) MODE="release" ;;
        --check)   CARGO_CMD="check"; MODE="check" ;;
    esac
done

info()  { echo -e "\033[32m[metal-build]\033[0m $*"; }
error() { echo -e "\033[31m[metal-build] ERROR:\033[0m $*" >&2; exit 1; }

# macOS only
[[ "$(uname -s)" == "Darwin" ]] || error "Metal builds require macOS. On Linux, use Dockerfile.gpu for CUDA."

# Xcode CLT
command -v clang >/dev/null 2>&1 || error "clang not found - run: xcode-select --install"

# cmake
command -v cmake >/dev/null 2>&1 || error "cmake not found - run: brew install cmake"

# cargo
command -v cargo >/dev/null 2>&1 || error "cargo not found - install Rust via rustup.rs"

info "macOS $(sw_vers -productVersion) | $(uname -m)"
info "cmake  $(cmake --version | head -1)"
info "rustc  $(rustc --version)"
info ""

# build

cd "${REPO_ROOT}"

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking with Metal feature..."
    cargo check \
        --workspace \
        --features maranode-inference/metal
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranoded with Metal GPU acceleration (${MODE})..."
cargo build \
    ${RELEASE_FLAG} \
    --bin maranoded \
    --features maranode-inference/metal

info ""
BINARY="target/${MODE}/maranoded"
info "Binary: ${REPO_ROOT}/${BINARY}"
info "Size:   $(du -sh "${BINARY}" | cut -f1)"
info ""
info "To run with Metal GPU:"
info "  ./${BINARY} --device auto --no-isolation --log-level debug"
info ""
info "To verify Metal is active, look for:"
info "  llama.cpp backend initialised (device=metal, n_gpu_layers=9999)"
