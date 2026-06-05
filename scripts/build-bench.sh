#!/usr/bin/env bash
# build the maranode-bench binary.
#
# usage:
#   ./scripts/build-bench.sh              # debug build, CPU backend
#   ./scripts/build-bench.sh --release    # release build (optimised)
#   ./scripts/build-bench.sh --metal      # release build, Apple Metal
#   ./scripts/build-bench.sh --cuda       # release build, NVIDIA CUDA
#   ./scripts/build-bench.sh --rocm       # release build, AMD ROCm
#   ./scripts/build-bench.sh --vulkan     # release build, Vulkan
#   ./scripts/build-bench.sh --openvino   # release build, Intel OpenVINO / NPU
#   ./scripts/build-bench.sh --check      # type-check only (fast)
#
# after building, run with:
#   ./target/release/maranode-bench --model /path/to/model.gguf
#   ./target/release/maranode-bench --model /path/to/model.gguf --device gpu --runs 20

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# parse arguments
MODE="debug"
CARGO_CMD="build"
FEATURE="cpu"

for arg in "$@"; do
    case "$arg" in
        --release)  MODE="release" ;;
        --metal)    MODE="release"; FEATURE="metal" ;;
        --cuda)     MODE="release"; FEATURE="cuda" ;;
        --rocm)     MODE="release"; FEATURE="rocm" ;;
        --vulkan)   MODE="release"; FEATURE="vulkan" ;;
        --openvino) MODE="release"; FEATURE="openvino" ;;
        --check)    CARGO_CMD="check"; MODE="check" ;;
        --help)
            echo "Usage: $0 [--release] [--metal|--cuda|--rocm|--vulkan|--openvino] [--check]"
            exit 0 ;;
    esac
done

info()  { echo -e "\033[32m[bench-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[bench-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[bench-build] ERROR:\033[0m $*" >&2; exit 1; }

command -v cargo >/dev/null 2>&1 || error "cargo not found - install Rust from https://rustup.rs"
command -v cmake  >/dev/null 2>&1 || warn  "cmake not found - required for llama.cpp C++ build"

if [[ "${FEATURE}" == "openvino" ]]; then
    OPENVINO_INSTALL_DIR="${OPENVINO_INSTALL_DIR:-/opt/intel/openvino_2024}"
    SETUPVARS="${OPENVINO_INSTALL_DIR}/setupvars.sh"
    if [[ -f "${SETUPVARS}" ]]; then
        info "Sourcing OpenVINO environment: ${SETUPVARS}"
        # shellcheck disable=SC1090
        source "${SETUPVARS}"
    else
        warn "OpenVINO setupvars.sh not found at ${SETUPVARS}."
        warn "Build may fail if OpenVINO headers are missing."
    fi
fi

cd "${REPO_ROOT}"

info "Target: maranode-bench | Backend: ${FEATURE} | Mode: ${MODE}"
info ""

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking maranode-bench..."
    cargo check \
        --package maranode-bench \
        --no-default-features \
        --features "${FEATURE}"
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranode-bench (${MODE}, ${FEATURE})..."
cargo build \
    ${RELEASE_FLAG} \
    --package maranode-bench \
    --no-default-features \
    --features "${FEATURE}"

BINARY="target/${MODE}/maranode-bench"
info ""
info "Binary : ${REPO_ROOT}/${BINARY}"
info "Size   : $(du -sh "${BINARY}" 2>/dev/null | cut -f1 || echo 'n/a')"
info ""
info "Quick start:"
info "  ./${BINARY} --model /path/to/model.gguf"
info "  ./${BINARY} --model /path/to/model.gguf --runs 20 --output json"
info "  ./${BINARY} --model /path/to/model.gguf --device gpu --runs 20"
info "  ./${BINARY} --help"
