#!/usr/bin/env bash
# build maranoded with Intel OpenVINO / NPU acceleration.
#
# Targets:
#   - Intel Core Ultra "Meteor Lake" NPU (via OpenVINO NPU plugin)
#   - Intel Arc GPU series (via OpenVINO GPU plugin)
#   - Intel CPU (via OpenVINO CPU plugin - faster than llama.cpp on some models)
#
# Requirements:
#   - Linux x86_64 or Windows x86_64
#   - Intel OpenVINO Runtime 2024.x:
#       https://docs.openvino.ai/latest/openvino_docs_install_guides_overview.html
#   - cmake 3.14+
#   - Rust 1.86+
#
# usage:
#   ./scripts/build-openvino.sh            # debug build
#   ./scripts/build-openvino.sh --release  # release build
#   ./scripts/build-openvino.sh --check    # type-check only (fast)
#
# environment:
#   OPENVINO_INSTALL_DIR  Path to OpenVINO installation
#                         (default: /opt/intel/openvino_2024)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

OPENVINO_INSTALL_DIR="${OPENVINO_INSTALL_DIR:-/opt/intel/openvino_2024}"

MODE="debug"
CARGO_CMD="build"

for arg in "$@"; do
    case "$arg" in
        --release) MODE="release" ;;
        --check)   CARGO_CMD="check"; MODE="check" ;;
    esac
done

info()  { echo -e "\033[32m[openvino-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[openvino-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[openvino-build] ERROR:\033[0m $*" >&2; exit 1; }

[[ "$(uname -s)" == "Linux" ]] || error "OpenVINO builds currently require Linux."

command -v cmake  >/dev/null 2>&1 || error "cmake not found"
command -v cargo  >/dev/null 2>&1 || error "cargo not found - install Rust via rustup.rs"

SETUPVARS="${OPENVINO_INSTALL_DIR}/setupvars.sh"
if [[ -f "${SETUPVARS}" ]]; then
    info "Sourcing OpenVINO environment from ${SETUPVARS}"
    source "${SETUPVARS}"
elif [[ -d "${OPENVINO_INSTALL_DIR}" ]]; then
    warn "OpenVINO found at ${OPENVINO_INSTALL_DIR} but setupvars.sh is missing."
    warn "Continuing - cmake may not find OpenVINO libraries."
else
    warn "OpenVINO not found at ${OPENVINO_INSTALL_DIR}."
    warn "Install from: https://docs.openvino.ai/latest/openvino_docs_install_guides_overview.html"
    warn "Or set OPENVINO_INSTALL_DIR to your installation path."
    warn "Continuing anyway - build will fail if OpenVINO headers are missing."
fi

info "Linux $(uname -r)"
info "cmake  $(cmake --version | head -1)"
info "rustc  $(rustc --version)"
info ""

if lspci 2>/dev/null | grep -qi "neural\|npu\|meteor lake\|arrow lake"; then
    info "Intel NPU detected in system."
elif lspci 2>/dev/null | grep -qi "intel.*display\|intel.*arc\|intel.*graphics"; then
    info "Intel GPU/iGPU detected - OpenVINO GPU plugin will be used."
else
    warn "No Intel NPU/GPU detected - OpenVINO CPU plugin will be used as fallback."
fi

# build

cd "${REPO_ROOT}"

if [[ "${CARGO_CMD}" == "check" ]]; then
    info "Type-checking with OpenVINO feature..."
    cargo check \
        --workspace \
        --features maranode-inference/openvino
    info "Type-check passed ✓"
    exit 0
fi

RELEASE_FLAG=""
[[ "${MODE}" == "release" ]] && RELEASE_FLAG="--release"

info "Building maranoded with Intel OpenVINO acceleration (${MODE})..."
cargo build \
    ${RELEASE_FLAG} \
    --bin maranoded \
    --features maranode-inference/openvino

info ""
BINARY="target/${MODE}/maranoded"
info "Binary: ${REPO_ROOT}/${BINARY}"
info "Size:   $(du -sh "${BINARY}" | cut -f1)"
info ""
info "To run with OpenVINO NPU:"
info "  ./${BINARY} --device npu --no-isolation --log-level debug"
info ""
info "To run with OpenVINO on CPU or GPU (auto-select):"
info "  ./${BINARY} --device auto --no-isolation --log-level debug"
info ""
info "Verify OpenVINO is active - look for:"
info "  llama.cpp backend initialised (device=npu, n_gpu_layers=9999)"
