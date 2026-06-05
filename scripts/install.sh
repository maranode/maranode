#!/usr/bin/env bash
# Maranode installer - https://github.com/maranode/maranode
# Usage: curl -sSL https://get.maranode.com | sh
set -euo pipefail

REPO="maranode/maranode"
INSTALL_DIR="/usr/local/bin"
DATA_DIR="/var/lib/maranode"
LOG_DIR="/var/log/maranode"
SERVICE_DIR="/lib/systemd/system"

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; RESET='\033[0m'
info()  { printf "${GREEN}->${RESET} %s\n" "$*"; }
warn()  { printf "${YELLOW}!${RESET} %s\n" "$*"; }
fatal() { printf "${RED}✗${RESET} %s\n" "$*" >&2; exit 1; }

OS=$(uname -s)
ARCH=$(uname -m)

case "$OS" in
  Linux)  ;;
  Darwin) fatal "macOS detected. Use Homebrew: brew install maranode/tap/maranode" ;;
  *)      fatal "Unsupported OS: $OS" ;;
esac

case "$ARCH" in
  x86_64)  TARGET="x86_64-unknown-linux-gnu"  ;;
  aarch64) TARGET="aarch64-unknown-linux-gnu" ;;
  *)       fatal "Unsupported architecture: $ARCH" ;;
esac

command -v curl > /dev/null 2>&1 || fatal "curl is required but not installed"
command -v tar  > /dev/null 2>&1 || fatal "tar is required but not installed"


info "Fetching latest release..."
TAG=$(curl -sSf "https://api.github.com/repos/${REPO}/releases/latest" \
  | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"\(.*\)".*/\1/')
[ -n "$TAG" ] || fatal "Could not determine latest release tag"
info "Latest release: $TAG"

ARCHIVE="maranode-${TAG}-${TARGET}.tar.gz"
BASE_URL="https://github.com/${REPO}/releases/download/${TAG}"

# download

TMP=$(mktemp -d)
trap 'rm -rf "$TMP"' EXIT

info "Downloading $ARCHIVE..."
curl -sSfL "${BASE_URL}/${ARCHIVE}"        -o "${TMP}/${ARCHIVE}"
curl -sSfL "${BASE_URL}/${ARCHIVE}.sha256" -o "${TMP}/${ARCHIVE}.sha256"
curl -sSfL "${BASE_URL}/${ARCHIVE}.sig"    -o "${TMP}/${ARCHIVE}.sig"
curl -sSfL "${BASE_URL}/${ARCHIVE}.crt"    -o "${TMP}/${ARCHIVE}.crt"

info "Verifying checksum..."
(cd "$TMP" && sha256sum -c "${ARCHIVE}.sha256") || fatal "Checksum verification failed"

if command -v cosign > /dev/null 2>&1; then
  info "Verifying cosign signature..."
  COSIGN_EXPERIMENTAL=1 cosign verify-blob \
    --certificate "${TMP}/${ARCHIVE}.crt" \
    --signature   "${TMP}/${ARCHIVE}.sig" \
    --certificate-identity-regexp "https://github.com/${REPO}/.github/workflows/" \
    --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
    "${TMP}/${ARCHIVE}" \
    || fatal "Signature verification failed"
else
  warn "cosign not found - skipping signature verification."
  warn "Install cosign: https://docs.sigstore.dev/cosign/installation/"
fi


info "Installing binaries to ${INSTALL_DIR}..."
tar -xzf "${TMP}/${ARCHIVE}" -C "$TMP"

SUDO=""
[ "$(id -u)" -ne 0 ] && SUDO="sudo"

$SUDO install -m 755 "${TMP}/maranoded" "${INSTALL_DIR}/maranoded"
$SUDO install -m 755 "${TMP}/maranode"  "${INSTALL_DIR}/maranode"


if command -v systemctl > /dev/null 2>&1; then
  info "Setting up systemd service..."

  $SUDO useradd --system --no-create-home --home "${DATA_DIR}" \
       --shell /usr/sbin/nologin maranode 2>/dev/null || true

  $SUDO mkdir -p "${DATA_DIR}" "${LOG_DIR}"
  $SUDO chown maranode:maranode "${DATA_DIR}" "${LOG_DIR}"

  $SUDO tee "${SERVICE_DIR}/maranoded.service" > /dev/null <<EOF
[Unit]
Description=Maranode AI Inference Daemon
After=network.target

[Service]
Type=simple
User=maranode
Group=maranode
ExecStart=${INSTALL_DIR}/maranoded
Restart=on-failure
RestartSec=5
NoNewPrivileges=true
PrivateTmp=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=${DATA_DIR} ${LOG_DIR}

[Install]
WantedBy=multi-user.target
EOF

  $SUDO systemctl daemon-reload
  $SUDO systemctl enable maranoded
  $SUDO systemctl start  maranoded || true
fi


printf "\n${GREEN}✓${RESET} Maranode ${TAG} installed.\n\n"
printf "  Import a model:  maranode model import /path/to/model.gguf --name mymodel --tag latest\n"
printf "  Open the UI:     http://localhost:11984/ui\n\n"
