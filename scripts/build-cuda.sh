#!/usr/bin/env bash
# build maranoded with NVIDIA CUDA GPU acceleration.
#
# Requirements:
#   - Linux x86_64 (CUDA does not support macOS)
#   - NVIDIA driver >= 525 (CUDA 12.x)
#   - CUDA Toolkit 12.x  (nvcc, libcuda, libcublas)
#   - cmake 3.14+
#   - Rust 1.86+
#
# usage:
#   ./scripts/build-cuda.sh            # debug build
#   ./scripts/build-cuda.sh --release  # release build
#   ./scripts/build-cuda.sh --check    # type-check only (fast)

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

info()  { echo -e "\033[32m[cuda-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[cuda-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[cuda-build] ERROR:\033[0m $*" >&2; exit 1; }

# check requirements

[[ "$(uname -s)" == "Linux" ]] || error "CUDA builds require Linux. Use build-metal.sh on macOS."

command -v nvcc   >/dev/null 2>&1 || error "nvcc not found - install CUDA Toolkit: https://developer.nvidia.com/cuda-downloads"
command -v cmake  >/dev/null 2>&1 || error "cmake not found - run: apt install cmake"
command -v cargo  >/dev/null 2>&1 || error "cargo not found - install Rust via rustup.rs"

CUDA_VERSION=$(nvcc --version | grep "release" | awk '{print $6}' | cut -c2-)
info "Linux $(uname -r) | CUDA ${CUDA_VERSION}"
info "cmake  $(cmake --version | head -1)"
info "rustc  $(rustc --version)"
info ""

# Warn if NVIDIA driver is not loaded (build will succeed but won't run).
if ! command -v nvidia-smi >/dev/null 2>&1; then
    warn "nvidia-smi not found - the binary will build but may not run on this machine."
fi

# build

cd "${REPO_ROOT}"

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking with CUDA feature..."
    cargo check \
        --workspace \
        --features maranode-inference/cuda
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranoded with CUDA GPU acceleration (${MODE})..."
cargo build \
    ${RELEASE_FLAG} \
    --bin maranoded \
    --features maranode-inference/cuda

info ""
BINARY="target/${MODE}/maranoded"
info "Binary: ${REPO_ROOT}/${BINARY}"
info "Size:   $(du -sh "${BINARY}" | cut -f1)"
info ""
info "To run with CUDA:"
info "  ./${BINARY} --device gpu --no-isolation --log-level debug"
info ""
info "Verify CUDA is active - look for:"
info "  llama.cpp backend initialised (device=gpu, n_gpu_layers=9999)"
