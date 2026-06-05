# Installing Maranode

## Quick install (Linux)

```bash
curl -sSL https://get.maranode.com | sh
```

The installer detects your architecture (x86_64 or aarch64), downloads the signed release binary, verifies the SHA256 checksum and cosign signature, installs to `/usr/local/bin`, and configures a systemd service.

---

## Package managers

### Debian / Ubuntu

```bash
# Add the repository (one-time setup)
curl -sSL https://maranode.github.io/maranode/apt/maranode-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/maranode-archive-keyring.gpg > /dev/null

echo "deb [signed-by=/usr/share/keyrings/maranode-archive-keyring.gpg] \
  https://maranode.github.io/maranode/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/maranode.list

sudo apt update && sudo apt install maranode
```

### macOS (Homebrew)

```bash
brew tap maranode/maranode
brew install maranode
```

The formula uses Metal-accelerated builds on Apple Silicon.

---

## Manual installation

```bash
TAG=$(curl -sSf https://api.github.com/repos/maranode/maranode/releases/latest \
  | grep tag_name | head -1 | sed 's/.*"\(.*\)".*/\1/')
ARCHIVE=maranode-${TAG}-x86_64-unknown-linux-gnu.tar.gz
BASE=https://github.com/maranode/maranode/releases/download/${TAG}

curl -LO ${BASE}/${ARCHIVE}
curl -LO ${BASE}/${ARCHIVE}.sha256
curl -LO ${BASE}/${ARCHIVE}.sig
curl -LO ${BASE}/${ARCHIVE}.crt

# Verify checksum
sha256sum -c ${ARCHIVE}.sha256

# Verify cosign signature
COSIGN_EXPERIMENTAL=1 cosign verify-blob \
  --certificate ${ARCHIVE}.crt \
  --signature   ${ARCHIVE}.sig \
  --certificate-identity-regexp "https://github.com/maranode/maranode/.github/workflows/" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  ${ARCHIVE}

# Install
tar xzf ${ARCHIVE}
sudo install -m 755 maranoded maranode /usr/local/bin/
```

---

## Docker

```bash
docker run -d \
  --name maranode \
  -p 11984:11984 \
  -v maranode-data:/var/lib/maranode \
  maranode/runtime:latest
```

---

## systemd service

After installation the daemon is managed via systemd:

```bash
systemctl enable --now maranoded
systemctl status maranoded
journalctl -u maranoded -f
```

---

## Post-install

```bash
maranode verify health
maranode model import /path/to/model.gguf --name llama3.2 --tag 3b
open http://localhost:11984/ui
```

---

## Supported platforms

| Platform | Architecture | Backend |
|----------|-------------|---------|
| Ubuntu 22.04+ / Debian 12 | x86_64, aarch64 | CPU, CUDA, ROCm, OpenVINO (NPU/iGPU), Ryzen AI (XDNA NPU) |
| macOS 13+ (Apple Silicon) | aarch64 | Metal |
| macOS 13+ (Intel) | x86_64 | CPU |

Kernel ≥ 5.15 required on Linux. Windows is not supported.

OpenVINO NPU/iGPU support requires an Intel Core Ultra processor and the [OpenVINO Runtime](https://docs.openvino.ai/install). Build with `make build-npu` and source `setupvars.sh` before running the daemon.

AMD Ryzen AI XDNA NPU support requires an AMD Ryzen AI processor and the [AMD Ryzen AI SDK](https://ryzenai.docs.amd.com). Build with `make build-ryzenai` and set `RYZENAI_INSTALL_PATH` before running the daemon. See `Development.md` for details on both.
