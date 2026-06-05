# Maranode

**Local LLM runtime that can't phone home.**

Run any GGUF model on your own hardware. OpenAI-compatible API. Network isolation enforced at the kernel level, not a config flag. HMAC-chained audit log ready for GDPR, HIPAA, and SOC 2. Single Rust binary — no Python, no Docker required.

Pre-alpha. API shape is mostly settled; hardening is ongoing.

---

## Why another local LLM runtime?

[Ollama](https://ollama.com) is excellent for development. But it is not designed for environments where you actually need to prove data never left the machine. Maranode is.

| | Ollama | LM Studio | Maranode |
|---|---|---|---|
| OpenAI-compatible API | ✓ | ✓ | ✓ |
| GPU / Metal acceleration | ✓ | ✓ | ✓ |
| Network air-gap (kernel-level) | — | — | ✓ |
| Tamper-evident audit log | — | — | ✓ |
| Compliance exports (GDPR, HIPAA…) | — | — | ✓ |
| Multi-tenant workspaces | — | — | ✓ |
| TPM attestation | — | — | ✓ (developing) |
| Single binary, no runtime deps | — | — | ✓ |

The network isolation is not "we disabled telemetry." It is an iptables OUTPUT chain default-DROP applied at startup, active-probed on every `maranode verify network` call, and verifiable with `iptables -L` and `tcpdump` without trusting Maranode at all.

The audit log is not a log file. It is an append-only HMAC chain — every entry contains the HMAC of the previous entry. A tampered or deleted entry breaks the chain and is detectable by anyone who has the HMAC key.

---

## Quick start

### macOS (Apple Silicon or Intel)

```bash
brew tap maranode/maranode
brew install maranode
```

```bash
maranode serve
```

### Linux (apt)

```bash
curl -sSL https://maranode.github.io/maranode/apt/maranode-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/maranode-archive-keyring.gpg > /dev/null

echo "deb [signed-by=/usr/share/keyrings/maranode-archive-keyring.gpg] \
  https://maranode.github.io/maranode/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/maranode.list

sudo apt update && sudo apt install maranode
sudo systemctl enable --now maranoded
```

### Linux (curl installer)

```bash
curl -sSL https://get.maranode.com | sh
```

Supports Ubuntu 22.04+, Debian 12, RHEL/Rocky/Alma 9, Alpine 3.19+, Fedora 39+, Arch.

### Build from source (any platform)

```bash
# Prerequisites: Rust 1.88+, CMake 3.14+
# macOS also needs Xcode CLT: xcode-select --install

git clone https://github.com/maranode/maranode && cd maranode
make build        # auto-detects Metal / CUDA / ROCm / CPU
```

Specific backends:

```bash
make build-cpu      # CPU only, always works
make build-metal    # Apple Metal (macOS)
make build-cuda     # NVIDIA CUDA
make build-rocm     # AMD ROCm
make build-npu      # Intel NPU via OpenVINO
```

---

## Get a model running

```bash
# Load a model from local file (recommended for air-gapped setups)
maranode model import /path/to/Llama-3.2-3B-Q4_K_M.gguf --name llama3.2 --tag 3b

# Or pull from Hugging Face (requires network; disabled in air-gap mode)
maranode model pull bartowski/Llama-3.2-3B-Instruct-GGUF/Llama-3.2-3B-Instruct-Q4_K_M.gguf \
  --name llama3.2 --tag 3b
```

```bash
maranode chat "Summarize the main GDPR obligations for data processors"
```

Web UI at `http://localhost:11984/ui`.

Point any OpenAI SDK at `http://localhost:11984/v1`:

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:11984/v1", api_key="ignored")
client.chat.completions.create(model="llama3.2:3b", messages=[{"role":"user","content":"Hello"}])
```

---

## Key features

**Network isolation** — iptables egress DROP by default. Models run completely offline after import. Verify yourself: `maranode verify network` runs an active TCP probe and also prints the raw iptables-save output so you can check with your own tools.

**Audit log** — every inference, model import, config change, and daemon start is written to an append-only HMAC-chained JSON Lines file. `maranode audit verify` checks the chain. Compliance exports for GDPR, HIPAA, SOC 2, and ISO 27001 are one command away.

**RAG (Retrieval-Augmented Generation)** — local embeddings, SQLite vector store, grounded answers with citations. Nothing leaves the machine. Disabled by default; enable with `--rag`.

**Workspaces** — isolated multi-tenant environments, each with its own API key, model allowlist, rate limit, system prompt, and audit log. Useful for separating departments or applications on one server.

**Content-addressed model store** — SHA-256 verified on every load. Two models sharing the same weights deduplicate automatically. A partial download is never visible as a usable model.

**Single binary** — `maranoded` (daemon) and `maranode` (CLI). No Python interpreter, no separate database process, no sidecar. State is SQLite + flat files; you can inspect every byte with `cat`.

---

## Docs

- [Development.md](Development.md) — build, GPU backends, benchmarking, troubleshooting
- [docs/install.md](docs/install.md) — installation and supported platforms
- [docs/usage.md](docs/usage.md) — CLI and HTTP API reference
- [docs/users.md](docs/users.md) — user accounts, SSO (OIDC, LDAP, SAML)
- [docs/workspaces.md](docs/workspaces.md) — workspace isolation
- [docs/compliance.md](docs/compliance.md) — audit log, compliance exports, evidence bundles
- [docs/document-intelligence.md](docs/document-intelligence.md) — PDF ingest and RAG
- [docs/verification.md](docs/verification.md) — how to verify network isolation and attestation
- [ARCHITECTURE.md](ARCHITECTURE.md) — design principles, components, trust model, threat model

---

## Status

The core inference path, model store, audit log, isolation layer, RAG, and workspace isolation are implemented. NPU acceleration, the full web UI, and TPM attestation are in active development.

---

## License

Apache 2.0. See [LICENSE](LICENSE).

## Author

[ondercsn](https://github.com/ondercsn)
