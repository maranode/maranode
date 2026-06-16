# Maranode

> **Local LLM runtime that can prove what it did — for environments where a data leak is not an option.**

Your model never calls home. Every inference produces a signed receipt, offline-verifiable by anyone. Egress is default-deny at the kernel level from the moment the daemon starts, and you can check it yourself with `iptables -L` and `tcpdump` — no need to trust us.

This is not another wrapper around llama.cpp. The OpenAI-compatible API, GPU/NPU acceleration, and single-binary install are table stakes — every tool has those. What Maranode adds is a tamper-evident audit chain, hardware-bound key sealing, grounding proofs for RAG answers, crypto-shred on workspace deletion, and a complete incident response workflow — all running on your own hardware, no cloud required.

**Built for:** healthcare (HIPAA), legal, finance, government, defense, any regulated environment that needs to answer an auditor.

---

## Proof, not promises

```bash
# Every inference returns a signed receipt.
# The receipt is Ed25519 over canonical JSON — verify it with any crypto library.
curl -s http://localhost:11984/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2:3b","messages":[{"role":"user","content":"Hello"}],"with_receipt":true}' \
  | jq .receipt

# Verify offline with the standalone binary — no daemon, no database.
maranode-verify receipt.json
# exit 0 = verified, exit 1 = failed

# Confirm the firewall yourself — no need to trust Maranode.
sudo iptables -L OUTPUT
maranode verify network
sudo tcpdump -i any -n not port 11984
# (run inference in parallel — you will see nothing leave the machine)
```

---

## How it compares

The table below is only the differences. Ollama is excellent for development; it was not designed for regulated deployments.

| | Ollama | LM Studio | **Maranode** |
|---|:---:|:---:|:---:|
| Default-deny egress, kernel level | — | — | ✓ |
| Verifiable by operator's own tools | — | — | ✓ |
| Tamper-evident HMAC-chained audit | — | — | ✓ |
| Signed inference receipts (Ed25519) | — | — | ✓ |
| Compliance exports (GDPR/HIPAA/SOC 2/ISO 27001) | — | — | ✓ |
| Crypto-shred on workspace deletion | — | — | ✓ |
| Multi-tenant workspaces | — | — | ✓ |
| Local RAG with grounding proof | — | — | ✓ |
| TPM key sealing, PCR attestation | — | — | ✓ |
| Incident response & legal hold | — | — | ✓ |
| Data classification + DLP enforcement | — | — | ✓ |
| SIEM integration (Splunk, Elastic, Sentinel, QRadar) | — | — | ✓ |

---

## Quick start

### macOS

```bash
brew tap maranode/maranode
brew install maranode
maranode serve
```

### Linux — one line

```bash
curl -sSL https://get.maranode.com | sh
```

Tested on Ubuntu 22.04+, Debian 12, RHEL/Rocky/Alma 9, Alpine 3.19+, Fedora 39+, Arch.

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
# Requirements: Rust 1.88+, CMake 3.14+
# macOS: xcode-select --install

git clone https://github.com/maranode/maranode && cd maranode
make build        # auto-detects available hardware
```

| Backend | Build command |
|---|---|
| CPU (always works) | `make build-cpu` |
| Apple Metal | `make build-metal` |
| NVIDIA CUDA | `make build-cuda` |
| AMD ROCm | `make build-rocm` |
| Vulkan | `make build-vulkan` |
| Intel NPU (OpenVINO) | `make build-npu` |
| AMD Ryzen AI (XDNA) | `make build-ryzenai` |

### Run a model

```bash
# Import from disk — no network, works in fully air-gapped installs.
maranode model import /path/to/Llama-3.2-3B-Q4_K_M.gguf --name llama3.2 --tag 3b

# Or pull from Hugging Face (needs whitelist mode first if air-gap is active).
maranode model pull bartowski/Llama-3.2-3B-Instruct-GGUF/Llama-3.2-3B-Instruct-Q4_K_M.gguf \
  --name llama3.2 --tag 3b

# Chat from the CLI.
maranode chat "Summarize the following contract clause: ..."

# Web UI.
open http://localhost:11984/ui
```

### Drop-in for OpenAI SDK

```python
from openai import OpenAI
client = OpenAI(base_url="http://localhost:11984/v1", api_key="ignored")
response = client.chat.completions.create(
    model="llama3.2:3b",
    messages=[{"role": "user", "content": "Hello"}]
)
```

---

## Features

### Inference runtime

- **OpenAI-compatible API** — `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`, `/v1/models`. Streaming (SSE) and non-streaming.
- **Hardware acceleration** — CPU (x86_64, aarch64), NVIDIA CUDA, AMD ROCm, Apple Metal, Vulkan. Auto-selects at startup.
- **Single binary** — `maranoded` (daemon) + `maranode` (CLI) + `maranode-verify` (offline verifier). No Python, no database server, no sidecar process.
- **Content-addressed model store** — SHA-256 verified on every load. Duplicate weights stored once. Partial downloads are never visible as usable models.
- **LRU model eviction** — loaded models are evicted under memory pressure without a restart.
- **Concurrent request queue** — configurable `max_parallel` slots and `max_queue_depth`; excess requests get HTTP 503 rather than timing out silently.
- **Quantization tooling** — `maranode model quant inspect <file>` shows GGUF quantization; `quant recommend` suggests a quantization from RAM and parameter count.

### Proof-carrying inference

Every inference produces a signed, offline-verifiable receipt binding the model identity (GGUF SHA-256), input hash, output hash, token counts, timestamp, environment fingerprint, and TPM PCR values. The signature is plain Ed25519 over canonical JSON.

- **Signed receipt per inference** — add `"with_receipt": true` to get it inline; it is also always written to the audit log.
- **Standalone verifier binary** — `maranode-verify receipt.json` checks the signature with no daemon, no database, no Maranode dependency.
- **Verify by hand** — any Python `cryptography` or `PyNaCl` install can verify the signature. Steps in `docs/receipt.md`.
- **Past receipt extraction** — `maranode audit prove <request_id>` retrieves the signed receipt for any past inference.
- **Reproducible inference** — `"deterministic": true` pins temperature=0, top_k=1, seed=0 for greedy decoding. With `--features deterministic-kernels`, the same input re-runs to the exact same bytes.
- **Audit replay** — `maranode audit replay <request_id>` re-runs the original inference and confirms output hash matches.

### Network isolation

- **Default-deny egress** — iptables OUTPUT chain default policy is DROP at daemon startup, not a config flag you can forget. Loopback and the API port are explicitly allowed. Nothing else leaves the machine.
- **Self-verifying** — `maranode verify network` runs an active TCP egress probe and dumps `iptables-save`. You can repeat this with your own tools without trusting Maranode.
- **Drift detection** — the daemon re-probes on a configurable interval. Firewall drift produces a `fail-closed` event in the audit log.
- **Whitelist mode** — `mode = "whitelist"` plus explicit `[[isolation.whitelist]]` blocks for specific host/port pairs, for deployments that must reach an internal mirror.
- **Continuous isolation attestation** — every egress probe result is signed and chained into the audit log, so you can show an auditor that the machine was isolated for the entire duration of a session.

### Audit, compliance and evidence

- **HMAC-chained audit log** — every event is chained with an HMAC. A deleted or modified entry breaks the chain. `maranode audit verify` checks the entire chain.
- **Compliance exports** — `maranode audit export --format [gdpr|hipaa|soc2|iso27001]` maps events to framework controls.
- **Signed evidence bundles** — `maranode audit bundle` produces a signed ZIP with the audit log, chain proof, signing key, and attestation report. Accepted by GDPR, HIPAA, and SOC 2 auditors.
- **Legal hold** — `maranode audit hold create` locks a time range in the audit log, preventing retention policies from deleting it. Active incidents automatically place a hold.
- **Retention policies** — configurable log age limits with configurable exceptions for holds.
- **Prompts not stored by default** — audit records the SHA-256 of the prompt, not the text. Full-content logging is an explicit opt-in.
- **SIEM forwarding** — `maranode audit forward <host:port>` streams events over TCP. Pre-built integrations for **Splunk, Elastic, Microsoft Sentinel, and IBM QRadar** in `siem/`.

### RAG and document intelligence

- **Fully local RAG** — ground answers in your documents. Local embeddings via the inference engine, SQLite vector store, brute-force cosine retrieval, cited answers. Nothing leaves the host.
- **Honest refusal** — when no chunk scores above the threshold and grounding is required, the model says "this information is not in the provided documents" instead of guessing.
- **Document ingestion** — PDF (with text extraction and OCR), plain text, and direct text input. Single file or batch multipart upload.
- **Table and structured data extraction** — extracts tables from PDFs into structured form before chunking.
- **Collections** — group documents into named collections. Separate untrusted uploads from curated knowledge.
- **Grounding proof** — the signed receipt records which document chunks were retrieved, so the grounding of any answer is independently verifiable.
- **RAG encrypted at rest** — in a workspace, chunk text is encrypted under the workspace key. Deleting the workspace key (crypto-shred) makes the RAG data unreadable.
- **Ingest policy** — `anyone` (default), `admin_only`, or `allowlist`. Controls who can write to the persistent store.
- **Inline extract without ingest** — `POST /v1/rag/extract` processes a file and returns text for immediate use without storing anything.

### Multi-tenant workspaces

- **Isolated tenants** — each workspace has its own API key, audit segment, model allowlist, system prompt, resource quotas (`max_concurrent_requests`, `max_models`, `max_memory_bytes`), and RAG collection.
- **Crypto-shred** — deleting a workspace deletes its data-encryption key (DEK). All encrypted data (RAG chunks, content logs) becomes unreadable without further deletion. Satisfies GDPR right to erasure without manual file hunting.
- **Network namespace lifecycle** — create and delete per-workspace Linux network namespaces. Routing inference through the namespace is in progress.
- **Management API** — `GET/POST/PUT/DELETE /v1/workspaces/:slug` (admin key required).

### Identity and authentication

- **Local user accounts** — username/password, hashed in SQLite. `maranode users create/list/set-password`.
- **API keys** — per-workspace bearer tokens.
- **OIDC** — standard OIDC login flow, any compliant provider.
- **SAML 2.0** — SP-initiated SSO.
- **LDAP / Active Directory** — login against an LDAP directory. (Group sync is in progress.)
- **Session management** — `GET /v1/sessions`, `DELETE /v1/sessions/:id`.
- **Per-IP rate limiting** on all auth endpoints.

### Data classification

- **Sensitivity labels** — assign `Public / Internal / Confidential / Restricted` labels to RAG collections.
- **Clearance enforcement** — if a workspace's clearance is below a collection's label, retrieval is blocked.
- **Violations audit** — blocked access attempts produce `DataClassificationViolation` events in the audit log.
- **DLP sync** — `maranode dlp sync --provider <p>` pulls labels from an external DLP system.

### TPM and hardware attestation

- **TPM 2.0 PCR read** — reads Platform Configuration Registers at startup via direct `/dev/tpm0` (no tpm2-tools dependency).
- **Binary self-hash** — hashes its own executable at startup; recorded in the audit log.
- **Key sealing to PCR policy** — `maranode tpm seal <purpose>` seals a key blob to a TPM PCR state. The key is only unsealable if PCR values match — i.e., the binary has not been tampered with.
- **TEE detection** — Intel TDX and AMD SEV-SNP. TEE measurements are incorporated into the audit chain.
- **Attestation report** — `maranode verify attest` builds a JSON report with binary hash, PCR values, audit chain status, and TEE presence, signed with the node key.
- **Key rotation** — `maranode tpm rotate <purpose>` re-seals to the current PCR state.
- **Recovery export** — `maranode tpm export-recovery` writes an encrypted recovery bundle for TPM failure or replacement.

### Behavioral integrity

- **Model baselines** — `maranode baseline create <model>` runs a calibration suite and stores the behavioral fingerprint. `maranode baseline check <model>` re-runs and confirms the model has not drifted.
- **Approval registry** — `maranode model approve <model>` records a signed approval before a model is allowed to run in a workspace. Approval events are chained into the audit log.
- **Air-gapped model registry** — models can be registered in an offline registry and approved before they reach an air-gapped installation, without the installation ever touching the internet.

### Incident response

- **Declare** — `maranode incident declare` ends all active sessions, freezes the audit log cryptographically, and opens an incident record.
- **Investigate** — `maranode incident investigate` creates a forensic snapshot of current runtime state.
- **Break-glass credentials** — `maranode incident bg-generate` creates time-limited emergency credentials. Each use is logged.
- **Legal hold auto-placed** — active incidents automatically prevent retention policies from pruning relevant audit entries.
- **Close** — `maranode incident close` restores normal operation and appends a close event to the audit chain.

### Operations

- **systemd service** — ships with a unit file; `sudo systemctl enable --now maranoded`.
- **Hot config reload** — `kill -HUP $(pgrep maranoded)` or `maranode admin config-reload`; most settings apply without a restart.
- **Unix socket** — CLI communicates via `/run/maranode/api.sock` (no TCP for local management).
- **Benchmark tool** — `maranode bench` measures tokens/sec, first-token latency, and memory for any model across devices.
- **Docker images** — pre-built images for each hardware backend.
- **Package repositories** — apt, dnf, pacman, Homebrew tap.

---

## Docs

| | |
|---|---|
| [Architecture](architecture.md) | Design, trust model, threat model |
| [Roadmap](roadmap.md) | What is built, what is next, what we will not do |
| [Handbook](handbook.md) | Full feature inventory, status-tagged against the source |
| [Installation](install.md) | Installation and supported platforms |
| [Usage](usage.md) | CLI and HTTP API reference |
| [Users & auth](users.md) | User accounts and SSO |
| [Workspaces](workspaces.md) | Workspace isolation |
| [Compliance](compliance.md) | Audit log, compliance exports, evidence bundles |
| [Signed receipts](receipt.md) | Signed receipts and offline verification |
| [Grounding proof](grounding.md) | RAG grounding proof |
| [Network verification](verification.md) | Network isolation and attestation |
| [Crypto-shred & erasure](erasure.md) | Crypto-shred and right to erasure |
| [Reproducible inference](reproducible-inference.md) | Deterministic mode and replay |
| [Document intelligence](document-intelligence.md) | PDF ingest and RAG |
| [Development](development.md) | Build, GPU backends, benchmarking, troubleshooting |

---

## License

Apache 2.0. See [LICENSE](LICENSE).

## Author

[ondercsn](https://github.com/ondercsn)
