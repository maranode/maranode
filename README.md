# Maranode

> **The local LLM runtime built for environments where a data leak is not an option.**

Most local AI tools are built for developers on laptops. Maranode is built for hospitals, law firms, financial institutions, and any organization where sensitive data must stay inside the building — and you need to *prove* it.

**What makes it different:**

- **You can prove what it did.** Every inference, model load, and config change goes into a tamper-evident, HMAC-chained audit log. One command verifies the whole chain; one command exports GDPR/HIPAA/SOC 2 evidence.
- **It fails closed.** Egress is default-deny from the moment the daemon starts (iptables OUTPUT DROP), and you can confirm it with your own tools — `iptables -L`, `tcpdump` — without trusting us. If isolation or the audit log cannot be guaranteed, inference does not run.
- **No telemetry, no phone-home.** No update checker, no usage beacon, ever. The binary talks to nothing unless you tell it to.

The rest is table stakes, and Maranode has it too: OpenAI-compatible API, GPU/NPU/Metal acceleration, a single binary with no external services. None of that is the reason to use it.

Pre-alpha. Core runtime is working; hardening is ongoing.

---

## Who this is for

If you are running AI on sensitive data and someone in your organization has asked *"but how do we know it is not sending anything out?"* — this is the answer.

Designed for: **healthcare (HIPAA), legal, finance, government, defense, any regulated environment that runs GDPR or ISO 27001 audits.**

---

## How it compares

Ollama and LM Studio already cover the basics — OpenAI-compatible API, GPU/NPU/Metal acceleration — and so does Maranode (a single binary, no external services). The table below is only the differences that matter for a regulated deployment.

| | Ollama | LM Studio | Maranode |
|---|:---:|:---:|:---:|
| **Default-deny egress, verifiable** | — | — | ✓ |
| **Tamper-evident audit log** | — | — | ✓ |
| **Compliance exports** (GDPR, HIPAA, SOC 2, ISO 27001) | — | — | ✓ |
| **Multi-tenant workspaces** | — | — | ✓ |
| **Built-in local RAG** (no external vector DB) | — | — | ✓ |
| **TPM attestation** (verify runtime integrity) | — | — | ✓ partial |

Ollama is excellent for development. It is not designed for environments where you need to prove — not just claim — that data never left the machine. Maranode is.

---

## Verify the isolation yourself

```bash
# Verify isolation yourself — no need to trust Maranode
sudo iptables -L OUTPUT
maranode verify network   # active TCP probe + iptables-save dump

# If you want raw confirmation:
sudo tcpdump -i any -n not port 11984
# (run inference — you will see nothing going out)
```

The OUTPUT chain default policy is DROP from the moment the daemon starts. Even if a library deep in the stack tries to phone home, the packet does not leave the machine.

---

## Quick start

### macOS (Apple Silicon or Intel)

```bash
brew tap maranode/maranode
brew install maranode
maranode serve
```

### Linux — one line

```bash
curl -sSL https://get.maranode.com | sh
```

Supports Ubuntu 22.04+, Debian 12, RHEL / Rocky / Alma 9, Alpine 3.19+, Fedora 39+, Arch.

### Linux — apt repository

```bash
curl -sSL https://maranode.github.io/maranode/apt/maranode-archive-keyring.gpg \
  | sudo tee /usr/share/keyrings/maranode-archive-keyring.gpg > /dev/null

echo "deb [signed-by=/usr/share/keyrings/maranode-archive-keyring.gpg] \
  https://maranode.github.io/maranode/apt stable main" \
  | sudo tee /etc/apt/sources.list.d/maranode.list

sudo apt update && sudo apt install maranode
sudo systemctl enable --now maranoded
```

### Build from source

```bash
# Rust 1.88+, CMake 3.14+
# macOS: xcode-select --install

git clone https://github.com/maranode/maranode && cd maranode
make build          # auto-detects Metal / CUDA / ROCm / OpenVINO / CPU
```

| Backend | Command |
|---|---|
| CPU (always works) | `make build-cpu` |
| Apple Metal | `make build-metal` |
| NVIDIA CUDA | `make build-cuda` |
| AMD ROCm | `make build-rocm` |
| Intel NPU (OpenVINO) | `make build-npu` |
| AMD Ryzen AI (XDNA) | `make build-ryzenai` |
| Vulkan | `make build-vulkan` |

---

## Run your first model

```bash
# Pull from Hugging Face (online — disable air-gap first if the daemon is running)
maranode model pull bartowski/Llama-3.2-3B-Instruct-GGUF/Llama-3.2-3B-Instruct-Q4_K_M.gguf \
  --name llama3.2 --tag 3b --quant Q4_K_M

# Or import from disk (offline — no network needed)
maranode model import /path/to/Llama-3.2-3B-Q4_K_M.gguf --name llama3.2 --tag 3b

# Ask something
maranode chat "Summarize the financial tables of first quarter"
```

Web UI at `http://localhost:11984/ui`

Any OpenAI SDK works — just change `base_url`:

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:11984/v1", api_key="ignored")
client.chat.completions.create(
    model="llama3.2:3b",
    messages=[{"role": "user", "content": "Hello"}]
)
```

---

## Core features

### Privacy and isolation

**Default-deny network isolation** — the iptables OUTPUT chain is set to DROP at daemon startup, not left to a config flag you can forget. Toggle it off explicitly when you need to pull a model, then back on. Check it any time with `iptables -L` and `tcpdump` — you do not have to trust Maranode to verify it.

**Prompts are not stored by default** — the audit log records the SHA-256 hash of a prompt, not the text. (A hash is a fingerprint, not encryption; the point is that the content is never written to disk at all unless you opt in.) Full-content logging is an explicit opt-in with its own retention controls.

**HMAC-chained audit log** — every event (inference, model import, config reload, daemon start/stop) is chained with an HMAC. A deleted or modified entry breaks the chain. `maranode audit verify` checks the entire chain in one command.

**Compliance exports** — export the audit log as GDPR Article 30, HIPAA access log, SOC 2 security events, or ISO 27001 event log. Download a signed ZIP evidence bundle for auditors. Everything available via CLI, HTTP API, and web UI.

### Intelligence

**RAG (Retrieval-Augmented Generation)** — ground model answers in your own documents. Local embeddings, SQLite vector store, brute-force cosine retrieval, cited answers. Nothing leaves the machine. Start with `maranoded --rag`, import an embedding model, and add documents. When no relevant chunk is found, the model says so — it does not guess.

**Multi-tenant workspaces** — isolated environments within one daemon. Each workspace has its own API key, model allowlist, rate limit, system prompt, and audit log segment. Useful for hospitals separating departments, law firms separating clients, or SaaS products with multiple customers.

**Content-addressed model store** — SHA-256 verified on every load. Two models with identical weights deduplicate automatically. A partial download is never visible as a usable model.

### Operations

**Single binary, no external services** — `maranoded` (daemon) and `maranode` (CLI). No Python interpreter, no database server, no sidecar processes. State is SQLite plus flat files, so a backup is a file copy and there is no extra service to secure or patch.

**Broad hardware support** — CPU (x86_64, aarch64), NVIDIA CUDA, AMD ROCm, Apple Metal, Intel NPU via OpenVINO, AMD Ryzen AI XDNA, Vulkan. Device selected automatically at startup.

**Hot config reload** — most settings apply without restarting the daemon: `kill -HUP $(pgrep maranoded)` or `maranode admin config-reload`.

---

## Docs

- [Development.md](Development.md) — build, GPU backends, benchmarking, troubleshooting
- [docs/install.md](docs/install.md) — installation and supported platforms
- [docs/usage.md](docs/usage.md) — CLI and HTTP API reference
- [docs/users.md](docs/users.md) — user accounts, SSO (OIDC, LDAP, SAML)
- [docs/workspaces.md](docs/workspaces.md) — workspace isolation
- [docs/compliance.md](docs/compliance.md) — audit log, compliance exports, evidence bundles
- [docs/document-intelligence.md](docs/document-intelligence.md) — PDF ingest and RAG
- [docs/verification.md](docs/verification.md) — network isolation and attestation verification
- [ARCHITECTURE.md](ARCHITECTURE.md) — design, trust model, threat model
- [ROADMAP.md](ROADMAP.md) — what is done, what is next, what we will not do
---

## Status

Phase 0. Core inference, model store, audit log, network isolation, RAG, and workspace isolation are implemented. NPU acceleration and the full web UI are in active development; TPM attestation is partial (binary hash and PCR read work today).

---

## License

Apache 2.0. See [LICENSE](LICENSE).

## Author

[ondercsn](https://github.com/ondercsn)
