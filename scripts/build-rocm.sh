#!/usr/bin/env bash
# build maranoded with AMD ROCm/HIP GPU acceleration.
#
# Requirements:
#   - Linux x86_64 (ROCm does not support macOS or Windows)
#   - AMD GPU with ROCm support (RX 6000/7000 series, Instinct MI series)
#   - ROCm 5.7+ - https://rocm.docs.amd.com/en/latest/deploy/linux/
#   - cmake 3.14+
#   - Rust 1.86+
#
# Supported AMD GPUs:
#   - Radeon RX 6600/6700/6800/6900 XT (gfx1031/1032)
#   - Radeon RX 7600/7700/7800/7900 XT (gfx1100/1101/1102)
#   - Radeon PRO W7900 / MI300X (gfx1100/gfx942)
#
# usage:
#   ./scripts/build-rocm.sh            # debug build
#   ./scripts/build-rocm.sh --release  # release build
#   ./scripts/build-rocm.sh --check    # type-check only (fast)
#
# environment:
#   ROCM_PATH  Path to ROCm installation (default: /opt/rocm)
#   AMDGPU_TARGETS  GPU target architectures, comma-separated
#                   (e.g. "gfx1100,gfx1030" - default: auto-detected)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
ROCM_PATH="${ROCM_PATH:-/opt/rocm}"

MODE="debug"
CARGO_CMD="build"

for arg in "$@"; do
    case "$arg" in
        --release) MODE="release" ;;
        --check)   CARGO_CMD="check"; MODE="check" ;;
    esac
done

info()  { echo -e "\033[32m[rocm-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[rocm-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[rocm-build] ERROR:\033[0m $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || error "ROCm builds require Linux."

[[ -d "${ROCM_PATH}" ]] || error "ROCm not found at ${ROCM_PATH}. Install from: https://rocm.docs.amd.com"

command -v cmake  >/dev/null 2>&1 || error "cmake not found - run: apt install cmake"
command -v cargo  >/dev/null 2>&1 || error "cargo not found - install Rust via rustup.rs"

export PATH="${ROCM_PATH}/bin:${PATH}"
export CMAKE_PREFIX_PATH="${ROCM_PATH}:${CMAKE_PREFIX_PATH:-}"

ROCM_VERSION="unknown"
if [[ -f "${ROCM_PATH}/.info/version" ]]; then
    ROCM_VERSION=$(cat "${ROCM_PATH}/.info/version")
elif command -v hipconfig >/dev/null 2>&1; then
    ROCM_VERSION=$(hipconfig --version 2>/dev/null || echo "unknown")
fi

info "Linux $(uname -r) | ROCm ${ROCM_VERSION} at ${ROCM_PATH}"
info "cmake  $(cmake --version | head -1)"
info "rustc  $(rustc --version)"
info ""

if [[ -z "${AMDGPU_TARGETS:-}" ]]; then
    if command -v rocminfo >/dev/null 2>&1; then
        AMDGPU_TARGETS=$(rocminfo 2>/dev/null \
            | grep "Name:.*gfx" | awk '{print $2}' \
            | sort -u | tr '\n' ',' | sed 's/,$//')
        if [[ -n "${AMDGPU_TARGETS}" ]]; then
            info "Auto-detected GPU targets: ${AMDGPU_TARGETS}"
        fi
    fi
fi

if [[ -n "${AMDGPU_TARGETS:-}" ]]; then
    export AMDGPU_TARGETS
    info "Building for GPU targets: ${AMDGPU_TARGETS}"
else
    warn "Could not auto-detect GPU targets - ROCm will use default targets."
    warn "Set AMDGPU_TARGETS=gfx1100 (for example) to target a specific GPU."
fi

cd "${REPO_ROOT}"

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking with ROCm feature..."
    cargo check \
        --workspace \
        --features maranode-inference/rocm
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranoded with AMD ROCm/HIP GPU acceleration (${MODE})..."
cargo build \
    ${RELEASE_FLAG} \
    --bin maranoded \
    --features maranode-inference/rocm

info ""
BINARY="target/${MODE}/maranoded"
info "Binary: ${REPO_ROOT}/${BINARY}"
info "Size:   $(du -sh "${BINARY}" | cut -f1)"
info ""
info "To run with ROCm:"
info "  ./${BINARY} --device gpu --no-isolation --log-level debug"
info ""
info "Verify ROCm is active - look for:"
info "  llama.cpp backend initialised (device=gpu, n_gpu_layers=9999)"
