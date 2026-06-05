#!/usr/bin/env bash
# build maranode docker images.
#
# usage:
#   ./scripts/build-docker.sh              # CPU image
#   ./scripts/build-docker.sh --gpu        # NVIDIA CUDA image
#   ./scripts/build-docker.sh --rocm       # AMD ROCm image
#   ./scripts/build-docker.sh --openvino   # Intel OpenVINO / NPU image
#   ./scripts/build-docker.sh --all        # All images
#   ./scripts/build-docker.sh --push       # Build + push to registry
#
# environment variables:
#   VERSION        Semver (default: read from Cargo.toml)
#   REGISTRY       Docker registry (default: docker.io/maranode)
#   PLATFORM       Build platform (default: linux/amd64)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

BUILD_CPU=true
BUILD_GPU=false
BUILD_ROCM=false
BUILD_OPENVINO=false
PUSH=false

for arg in "$@"; do
    case $arg in
        --gpu)      BUILD_CPU=false; BUILD_GPU=true ;;
        --rocm)     BUILD_CPU=false; BUILD_ROCM=true ;;
        --openvino) BUILD_CPU=false; BUILD_OPENVINO=true ;;
        --all)      BUILD_CPU=true; BUILD_GPU=true; BUILD_ROCM=true; BUILD_OPENVINO=true ;;
        --push)     PUSH=true ;;
        --help)
            echo "Usage: $0 [--gpu] [--rocm] [--openvino] [--all] [--push]"
            exit 0 ;;
    esac
done

VERSION="${VERSION:-}"
if [ -z "${VERSION}" ]; then
    VERSION=$(grep '^version' "${REPO_ROOT}/Cargo.toml" | head -1 | sed 's/.*= *"\(.*\)"/\1/')
fi
REGISTRY="${REGISTRY:-docker.io/maranode}"
PLATFORM="${PLATFORM:-linux/amd64}"

info()  { echo -e "\033[32m[docker-build]\033[0m $*"; }
warn()  { echo -e "\033[33m[docker-build]\033[0m $*" >&2; }
error() { echo -e "\033[31m[docker-build] ERROR:\033[0m $*" >&2; exit 1; }

command -v docker >/dev/null 2>&1 || error "docker not found"

# CPU image
build_cpu() {
    info "Building CPU image (v${VERSION})..."
    docker build \
        --platform "${PLATFORM}" \
        --file "${REPO_ROOT}/Dockerfile" \
        --tag "${REGISTRY}/runtime:latest" \
        --tag "${REGISTRY}/runtime:${VERSION}" \
        --tag "${REGISTRY}/runtime:cpu-${VERSION}" \
        --label "org.opencontainers.image.version=${VERSION}" \
        --label "org.opencontainers.image.authors=ondercsn" \
        --label "org.opencontainers.image.created=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        "${REPO_ROOT}"

    info "CPU image ready: ${REGISTRY}/runtime:latest"

    if [ "${PUSH}" = "true" ]; then
        docker push "${REGISTRY}/runtime:latest"
        docker push "${REGISTRY}/runtime:${VERSION}"
        docker push "${REGISTRY}/runtime:cpu-${VERSION}"
    fi
}

# NVIDIA CUDA image
build_gpu() {
    info "Building NVIDIA CUDA image (v${VERSION})..."
    if ! docker info 2>/dev/null | grep -q nvidia; then
        warn "nvidia-container-toolkit not detected - GPU image will build but not run here."
    fi
    docker build \
        --platform "${PLATFORM}" \
        --file "${REPO_ROOT}/Dockerfile.gpu" \
        --tag "${REGISTRY}/runtime:gpu" \
        --tag "${REGISTRY}/runtime:gpu-${VERSION}" \
        --label "org.opencontainers.image.version=${VERSION}" \
        --label "org.opencontainers.image.authors=ondercsn" \
        --label "org.opencontainers.image.created=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        "${REPO_ROOT}"

    info "CUDA image ready: ${REGISTRY}/runtime:gpu"

    if [ "${PUSH}" = "true" ]; then
        docker push "${REGISTRY}/runtime:gpu"
        docker push "${REGISTRY}/runtime:gpu-${VERSION}"
    fi
}

# AMD ROCm image
build_rocm() {
    info "Building AMD ROCm image (v${VERSION})..."
    docker build \
        --platform "${PLATFORM}" \
        --file "${REPO_ROOT}/Dockerfile.rocm" \
        --tag "${REGISTRY}/runtime:rocm" \
        --tag "${REGISTRY}/runtime:rocm-${VERSION}" \
        --label "org.opencontainers.image.version=${VERSION}" \
        --label "org.opencontainers.image.authors=ondercsn" \
        --label "org.opencontainers.image.created=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        "${REPO_ROOT}"

    info "ROCm image ready: ${REGISTRY}/runtime:rocm"

    if [ "${PUSH}" = "true" ]; then
        docker push "${REGISTRY}/runtime:rocm"
        docker push "${REGISTRY}/runtime:rocm-${VERSION}"
    fi
}

# Intel OpenVINO / NPU image
build_openvino() {
    info "Building Intel OpenVINO / NPU image (v${VERSION})..."
    docker build \
        --platform "${PLATFORM}" \
        --file "${REPO_ROOT}/Dockerfile.openvino" \
        --tag "${REGISTRY}/runtime:npu" \
        --tag "${REGISTRY}/runtime:npu-${VERSION}" \
        --label "org.opencontainers.image.version=${VERSION}" \
        --label "org.opencontainers.image.authors=ondercsn" \
        --label "org.opencontainers.image.created=$(date -u +%Y-%m-%dT%H:%M:%SZ)" \
        "${REPO_ROOT}"

    info "OpenVINO image ready: ${REGISTRY}/runtime:npu"

    if [ "${PUSH}" = "true" ]; then
        docker push "${REGISTRY}/runtime:npu"
        docker push "${REGISTRY}/runtime:npu-${VERSION}"
    fi
}

# report image sizes
report_size() {
    info ""
    info "Image sizes:"
    docker images "${REGISTRY}/runtime" --format "  {{.Tag}}\t{{.Size}}" 2>/dev/null || true
}

info "Maranode Docker Build - v${VERSION}"
info "Registry: ${REGISTRY} | Platform: ${PLATFORM}"
info ""

[ "${BUILD_CPU}"      = "true" ] && build_cpu
[ "${BUILD_GPU}"      = "true" ] && build_gpu
[ "${BUILD_ROCM}"     = "true" ] && build_rocm
[ "${BUILD_OPENVINO}" = "true" ] && build_openvino

report_size

info ""
info "To start:"
[ "${BUILD_CPU}"      = "true" ] && info "  docker compose --profile cpu   up -d  # CPU"
[ "${BUILD_GPU}"      = "true" ] && info "  docker compose --profile gpu   up -d  # NVIDIA CUDA"
[ "${BUILD_ROCM}"     = "true" ] && info "  docker compose --profile rocm  up -d  # AMD ROCm"
[ "${BUILD_OPENVINO}" = "true" ] && info "  docker compose --profile npu   up -d  # Intel OpenVINO"
