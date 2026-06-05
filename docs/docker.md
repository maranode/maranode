# Maranode - Docker Guide

This document covers running Maranode with Docker, switching to GPU, and developing on a MacBook.

---

## Quick start

```bash
# build the image
./scripts/build-docker.sh

# start in CPU mode (air-gap enabled)
docker compose --profile cpu up -d

curl -s http://localhost:11984/health
```

---

## Scenarios

### 1. CPU - Production (air-gap enabled)

The default scenario. iptables rules are applied automatically.

```bash
docker compose --profile cpu up -d
```

**Important:** `network_mode: host` is used, meaning the container shares the host's network stack - iptables rules are applied directly to the host and block all outbound traffic. This is an intentional design decision: the host kernel's firewall enforces the air-gap, not the container's network namespace.

Model import:
```bash
# copy the model file from the host into the container
docker compose --profile cpu exec maranode \
  maranode model import /models/deepseek-r1-7b.gguf \
  --name deepseek-r1 --tag 7b
```

Audit log check:
```bash
docker compose --profile cpu exec maranode maranode audit verify
```

### 2. GPU - NVIDIA (production)

```bash
# nvidia-container-toolkit must be installed first
./scripts/build-docker.sh --gpu
docker compose --profile gpu up -d

# confirm GPU is in use
docker compose --profile gpu exec maranode-gpu nvidia-smi
```

**Host requirements:**

```bash
# Ubuntu/Debian
curl -fsSL https://nvidia.github.io/libnvidia-container/gpgkey | sudo gpg --dearmor \
  -o /usr/share/keyrings/nvidia-container-toolkit-keyring.gpg
curl -s -L https://nvidia.github.io/libnvidia-container/stable/deb/nvidia-container-toolkit.list \
  | sed 's#deb https://#deb [signed-by=/usr/share/keyrings/nvidia-container-toolkit-keyring.gpg] https://#g' \
  | sudo tee /etc/apt/sources.list.d/nvidia-container-toolkit.list
sudo apt-get update && sudo apt-get install -y nvidia-container-toolkit
sudo systemctl restart docker
```

### 3. MacBook - Development mode

`iptables` is not available on macOS. Isolation is disabled - this is for development only.

```bash
# build the image (first time 5–10 minutes)
./scripts/build-docker.sh

# start with the development profile
docker compose --profile dev up
```

Follow logs:
```bash
docker compose --profile dev logs -f
```

Test the API:
```bash
curl -s http://localhost:11984/health | python3 -m json.tool
curl -s http://localhost:11984/v1/models | python3 -m json.tool
```

---

## Importing models

The same method works for all profiles:

```bash
# 1. put the model file in the models/ directory
# (docker-compose.yml mounts it as /models inside the container)
mkdir -p models
cp /path/to/deepseek-r1-7b-q4.gguf models/

# 2. import
docker compose --profile cpu exec maranode \
  maranode model import /models/deepseek-r1-7b-q4.gguf \
  --name deepseek-r1 --tag 7b-q4

# 3. list
docker compose --profile cpu exec maranode maranode model list
```

---

## Verifying isolation (from inside the container)

```bash
# connect to the container
docker compose --profile cpu exec maranode sh

# view iptables rules
iptables -L -n -v

# try to reach the outside (should be blocked)
wget -T 3 https://google.com  # -> timeout ✓

# maranode's own verification
maranode verify network
```

---

## Environment variables

Create a `.env` file to configure:

```bash
# .env
MARANODE_LOG_LEVEL=debug
MODEL_DIR=/data/models # host directory containing model files
REGISTRY=docker.io/maranode  # image registry
```

| Variable | Default | Description |
|----------|---------|-------------|
| `MARANODE_DATA_DIR` | `/var/lib/maranode` | Model store and audit log |
| `MARANODE_BIND` | profile-dependent | Listen address |
| `MARANODE_LOG_LEVEL` | `info` | Log level |
| `MARANODE_NO_ISOLATION` | `0` | `1` -> force-disable isolation |
| `MODEL_DIR` | `./models` | Model directory on the host |

---

## Volume management

```bash
# list volumes
docker volume ls | grep maranode

# export the audit log to the host
docker run --rm \
  -v maranode_maranode-data:/data \
  -v $(pwd):/out \
  alpine cp /data/audit.jsonl /out/

# delete the volume entirely including models and logs
docker compose --profile cpu down -v
```

---

## Image sizes (targets)

| Image | Base | Target size |
|-------|------|-------------|
| `maranode/runtime:latest` | debian:bookworm-slim | < 100 MB |
| `maranode/runtime:gpu` | nvidia/cuda:12.4 | < 500 MB |

---

## Troubleshooting

### `iptables: Permission denied`

`--cap-add NET_ADMIN` is missing:
```bash
# with docker run
docker run --cap-add NET_ADMIN ...

# with docker compose. the cpu profile already adds it. verify with:
docker compose --profile cpu config | grep cap_add
```

### GPU not visible

```bash
# check if the driver installed on the host
nvidia-smi

# check if the toolkit installed
docker run --rm --gpus all nvidia/cuda:12.4.1-base-ubuntu22.04 nvidia-smi
```

### `exec format error` on MacBook

The MacBook is ARM architecture but the image was built for `linux/amd64`. Enable Rosetta emulation in Docker Desktop, or:
```bash
PLATFORM=linux/arm64 ./scripts/build-docker.sh
```

### Container starts but health check fails

```bash
docker logs maranode
# usually port conflict or data directory permission issue
```
