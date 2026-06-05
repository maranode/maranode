#!/usr/bin/env bash
# build maranoded with Vulkan GPU acceleration.
#
# Vulkan is a cross-platform GPU API supported on:
#   - Linux: NVIDIA (proprietary + Mesa), AMD (Mesa RADV), Intel (ANV)
#   - Windows: NVIDIA, AMD, Intel (not built here - use cross-compilation)
#   - macOS: via MoltenVK (translation layer over Metal)
#
# When to use this vs platform-specific builds:
#   - Use build-metal.sh on Apple Silicon (better performance than Vulkan/MoltenVK)
#   - Use build-cuda.sh on NVIDIA Linux (better performance than Vulkan)
#   - Use build-rocm.sh on AMD Linux (better performance than Vulkan)
#   - Use this script when you need cross-GPU portability on a single binary
#
# Requirements:
#   - Vulkan SDK: https://vulkan.lunarg.com/sdk/home
#   - cmake 3.14+
#   - Rust 1.86+
#
# usage:
#   ./scripts/build-vulkan.sh            # debug build
#   ./scripts/build-vulkan.sh --release  # release build
#   ./scripts/build-vulkan.sh --check    # type-check only (fast)

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

info()  { echo -e "\033[32m[vulkan-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[vulkan-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[vulkan-build] ERROR:\033[0m $*" >&2; exit 1; }

command -v cmake  >/dev/null 2>&1 || error "cmake not found"
command -v cargo  >/dev/null 2>&1 || error "cargo not found - install Rust via rustup.rs"

if ! command -v vulkaninfo >/dev/null 2>&1; then
    warn "vulkaninfo not found - Vulkan SDK may not be installed."
    warn "Install from: https://vulkan.lunarg.com/sdk/home"
    warn "Continuing anyway - build may fail if Vulkan headers are missing."
fi

OS="$(uname -s)"
info "${OS} $(uname -r)"
info "cmake  $(cmake --version | head -1)"
info "rustc  $(rustc --version)"

if [[ "${OS}" == "Darwin" ]]; then
    warn "On macOS, Vulkan runs via MoltenVK (a translation layer over Metal)."
    warn "For native performance on Apple Silicon, use build-metal.sh instead."
    if [[ -z "${VULKAN_SDK:-}" ]]; then
        VULKAN_SDK_CANDIDATES=(
            "$HOME/VulkanSDK/*/macOS"
            "/opt/homebrew/opt/molten-vk"
        )
        for candidate in "${VULKAN_SDK_CANDIDATES[@]}"; do
            for match in $candidate; do
                if [[ -d "$match" ]]; then
                    export VULKAN_SDK="$match"
                    info "Found Vulkan SDK at: ${VULKAN_SDK}"
                    break 2
                fi
            done
        done
    fi
fi

info ""


cd "${REPO_ROOT}"

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking with Vulkan feature..."
    cargo check \
        --workspace \
        --features maranode-inference/vulkan
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranoded with Vulkan GPU acceleration (${MODE})..."
cargo build \
    ${RELEASE_FLAG} \
    --bin maranoded \
    --features maranode-inference/vulkan

info ""
BINARY="target/${MODE}/maranoded"
info "Binary: ${REPO_ROOT}/${BINARY}"
info "Size:   $(du -sh "${BINARY}" | cut -f1)"
info ""
info "To run with Vulkan:"
info "  ./${BINARY} --device gpu --no-isolation --log-level debug"
info ""
info "Verify Vulkan is active - look for:"
info "  llama.cpp backend initialised (device=gpu, n_gpu_layers=9999)"
