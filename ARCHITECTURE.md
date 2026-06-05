# Maranode Architecture

Design notes: components, data flow, trust boundaries, threats. For contributors and security review.

---

## 1. Design principles

### 1.1 Fail closed, not open
If the firewall rule fails to load, no network access. If the audit log cannot be written, no inference. If the model checksum does not match, refuse to load. We never default to "permissive" in a security boundary.

### 1.2 Each layer is independently verifiable
A user with no knowledge of Maranode internals should be able to confirm isolation using standard Linux tools (`iptables -L`, `tcpdump`, `strace`, `lsof`). We do not invent custom verification mechanisms when standard ones exist.

### 1.3 Single binary, single process
The runtime is one Rust binary. No sidecar processes, no Python dependencies at runtime, no Docker requirement for the daemon itself. This minimizes attack surface and simplifies operations.

### 1.4 Boring storage
SQLite for metadata, flat files for blobs, append-only JSON Lines for audit logs. We do not run our own database. We do not require Redis or Postgres. A user can inspect every byte of state with `cat`.

### 1.5 No background phoning home
There is no update checker, no telemetry endpoint, no usage statistics. Maranode knows nothing about the outside world unless the operator explicitly tells it.

---

## 2. System Overview

Maranode is structured as four layers, each with a defined responsibility and a defined trust boundary:

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│   Layer 4: Interface                                         │
│   • CLI binary (maranode)                                    │
│   • Web UI (served by daemon at /ui)                         │
│   • HTTP API (OpenAI-compatible)                             │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 3: Orchestration                                     │
│   • Request router                                           │
│   • Model lifecycle manager                                  │
│   • Audit logger                                             │
│   • Workspace manager (Phase 2)                              │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 2: Inference                                         │
│   • llama.cpp via FFI                                        │
│   • Device backends (CPU, NPU, GPU)                          │
│   • Model store (content-addressed)                          │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 1: Isolation                                         │
│   • iptables egress rules                                    │
│   • Linux namespaces (Phase 2)                               │
│   • TPM attestation hooks (Phase 3)                          │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 0: Host OS                                           │
│   • Any modern Linux (kernel ≥ 5.15)                         │
│   • systemd or OpenRC                                        │
│   • Standard userspace                                       │
│                                                              │
└──────────────────────────────────────────────────────────────┘
```

**Why this layering matters:** Each layer can be replaced or omitted without affecting the others. An organization that already has a hardened OS uses only layers 1-4. Layer 1 can be disabled if the operator wants to use their own egress controls. The Phase 3 appliance replaces Layer 0 with a minimal Linux distribution.

---

## 3. Component Specifications

### 3.1 The Daemon (`maranoded`)

A single Rust binary running as a long-lived process. Started by systemd or equivalent.

**Responsibilities:**
- Listen on HTTP (default `127.0.0.1:11984`) and Unix socket (`/run/maranode/api.sock`)
- Manage model lifecycle (load, unload, evict under memory pressure)
- Route inference requests to the appropriate device backend
- Write audit log entries for every meaningful event
- Maintain isolation policy

**Technology choices:**
- **Rust** for memory safety in a process that handles untrusted input and crypto
- **Tokio** for async runtime - battle-tested, low overhead
- **Axum** for HTTP (simple, fast, well-supported)
- **rusqlite** for the metadata database (no separate DB process)

**What it does not do:**
- It does not directly load model weights into its own memory - that is delegated to llama.cpp via FFI
- It does not implement HTTP/TLS termination - operators put a reverse proxy in front if they need TLS
- It does not authenticate users in Core (Phase 2 / Enterprise feature)

### 3.2 The Inference Engine

llama.cpp, integrated via Rust FFI bindings. We use llama.cpp because:
- It is the most mature local inference engine
- It supports GGUF, the de facto standard model format
- It has the broadest hardware support (CPU SIMD, CUDA, ROCm, Metal, OpenVINO, etc.)
- Replacing it later, if needed, is a contained refactor

**Boundary:** llama.cpp runs in the same process as the daemon. Model weights are memory-mapped, not copied, so multiple loaded models share memory pages efficiently.

**Device backends** are selected at runtime based on what is available:
```
Priority order:
1. NPU (if present and the model is supported)
2. GPU (if present and the model is large enough to benefit)
3. CPU (always available, fallback)
```

We do not currently support distributing a single inference across multiple devices.

### 3.3 The Model Store

Content-addressed storage on the local filesystem, modeled on Docker's image storage but simpler.

**Layout:**
```
/var/lib/maranode/
├── models.db                    # SQLite metadata
└── blobs/
    ├── sha256-<hash1>           # Raw model data
    ├── sha256-<hash2>
    └── ...
```

**Properties:**
- Two models referencing the same weights deduplicate automatically
- Integrity check on every load (SHA-256 verification)
- Atomic operations: a partial download is never visible as a usable model
- Manifests stored in SQLite, weights stored as flat files (efficient mmap)

**Model identification:** `<name>:<tag>` (e.g., `llama3.2:3b-instruct`). This matches Ollama's convention so users do not need to learn new vocabulary.

### 3.4 The Audit Logger

Append-only JSON Lines with HMAC chaining. The most security-critical component besides the firewall.

**Each entry has the structure:**
```json
{
  "ts": "2026-05-21T14:23:45.123Z",
  "seq": 4231,
  "event": "inference.complete",
  "actor": "system",
  "data": {
    "model": "llama3.2:3b",
    "prompt_sha256": "abc...",
    "tokens_in": 142,
    "tokens_out": 487,
    "duration_ms": 3210,
    "device": "cpu"
  },
  "prev_hmac": "def456...",
  "hmac": "789abc..."
}
```

**Integrity model:**
- Every entry contains an HMAC of itself
- The HMAC includes the previous entry's HMAC (chain)
- The HMAC key is generated on first install, stored at `/var/lib/maranode/audit.key` (mode 0600)
- The Phase 3 appliance will seal this key to a TPM PCR

**What this proves:**
- A tampered entry breaks the chain (detectable)
- Deleted entries break the sequence (detectable)
- Cannot prevent an attacker with root from rewriting the entire log, but cannot do so undetectably from a previous known-good snapshot

**What prompts are logged:** SHA-256 of the prompt, not the prompt itself. We do not write user content to disk in the audit log. Operators who want full content logging enable it explicitly with a separate flag and retention controls.

### 3.5 The Isolation Layer

The point where Maranode differs most from Ollama.

**Air-gap mode** (default for fresh installs):
- iptables OUTPUT chain default policy: DROP
- INPUT chain default policy: DROP
- Loopback interface explicitly accepted
- Inbound on the configured API port from configured source addresses
- All other interfaces brought down (optional, behind a flag)

**Whitelist mode:**
- For environments needing model downloads or specific external services
- Operator explicitly allows specific destinations by hostname or IP
- Maranode maintains the iptables rules; manual modifications are detected and reported

**Verification:**
- `maranode verify network` runs an active probe against the firewall and reports state
- Verification uses standard Linux tools (iptables-save, ss, ip route) so the user can re-run them independently

**What this does not protect against:**
- A malicious operator with root privileges (any local AI tool has this problem)
- Hardware-level exfiltration (out of scope; requires trusted hardware)
- Side-channel attacks on shared infrastructure

### 3.6 Retrieval-Augmented Generation (optional)

A small local model has no reliable knowledge of an organization's private
documents and will confidently hallucinate when asked. RAG is the answer: rather
than asking the model to recall facts from its weights, Maranode retrieves the
relevant source text at query time and instructs the model to answer **only**
from that text, with citations. This is the only design that makes local-LLM
output trustworthy for medical, legal, and similar factual workloads.

This subsystem lives in the `maranode-rag` crate and is **disabled by default**.
When it is off, no RAG state is created and the inference path is identical to a
build without it.

**Properties, consistent with the rest of the architecture:**
- **Boring storage.** A single SQLite file (`rag.db`) next to the model store
  holds collections, documents, chunks, and embeddings (little-endian `f32`
  blobs). No external vector database, no extra daemon, no network.
- **Air-gap friendly.** Embeddings are produced by the same local inference
  engine that runs chat (via the `/v1/embeddings` path). Nothing leaves the host.
- **Exact retrieval.** Search is a brute-force cosine scan over a collection's
  chunks. At Maranode's target per-tenant document volumes this is fast and avoids
  the failure modes of an approximate index. The vector store is the single
  place to swap in an ANN index later if needed.
- **Auditable and deletable.** Ingestion and retrieval emit audit events (with
  the query hashed, never stored verbatim). Deleting a collection removes its
  documents and chunks - important for data-erasure obligations, which
  fine-tuning facts into model weights cannot satisfy.
- **Honest refusal.** When no chunk clears the similarity threshold and the
  caller requires grounding, the runtime returns
  "This information is not in the provided documents." instead of guessing.
- **Permission-gated writes, open reads.** Any user may extract text from a
  file for inline chat context (`POST /v1/rag/extract`) - nothing is stored.
  Permanently writing to the RAG store is governed by `rag.ingest_policy` (see
  below). This separates the "doctor uploads a patient report for this session"
  use case from "an administrator maintains the shared knowledge base".

**Why RAG and not fine-tuning for facts:** fine-tuning bakes data permanently
into the weights (a privacy and erasure problem), must be redone whenever the
data changes, and still hallucinates. RAG keeps knowledge in a database that is
current, scoped, auditable, and removable. Fine-tuning remains appropriate for
*behaviour* (tone, format, terminology), not for facts.

#### RAG ingest policy

The `rag.ingest_policy` setting controls who can write permanently to the
vector store. Three modes are supported:

| Mode | Who can ingest | Notes |
|---|---|---|
| `anyone` | No key required | Default. Suitable for single-user or loopback-only deployments. |
| `admin_only` | Admin key only | Recommended for multi-user. All writes require `Authorization: Bearer <admin_key>`. |
| `allowlist` | Admin key + listed keys | For service accounts (e.g. a document pipeline) that need write access without full admin rights. |

Ingest requests that fail the policy check receive `403 Forbidden`. The extract
endpoint (`/v1/rag/extract`) is **always open** - it only returns text, it never
writes anything.

#### Chat file attachments (ephemeral context)

When a user attaches a file in the chat view, the client calls
`POST /v1/rag/extract`, receives the extracted text, and injects it inline into
the prompt. The document is never stored in the RAG database. This means:

- Any user can ground a conversation in a document without needing ingest permission.
- The document is visible only in that conversation - no other user's query will
  retrieve it.
- There is no data-erasure obligation because nothing was persisted.

This is the intended flow for ephemeral, per-session context (patient reports,
draft contracts, meeting notes). The RAG store is for shared, curated knowledge
that every future query can retrieve.

---

## 4. Data Flow

### 4.1 Inference request flow

```
1. Client sends POST /v1/chat/completions to localhost:11984
                                  │
                                  ▼
2. Daemon receives request via Axum HTTP handler
                                  │
                                  ▼
3. Request validated, normalized (OpenAI -> internal format)
                                  │
                                  ▼
4. Audit log entry: inference.start
                                  │
                                  ▼
5. Model manager: is the model loaded? If not, load it.
                                  │
                                  ▼
6. Scheduler: select device backend (NPU/GPU/CPU)
                                  │
                                  ▼
7. llama.cpp generates tokens, streamed back through the daemon
                                  │
                                  ▼
8. Audit log entry: inference.complete
                                  │
                                  ▼
9. Response returned to client (streaming SSE or single JSON)
```

**Notable properties:**
- No external network calls at any point after model is loaded
- Audit entries written synchronously and fsynced before response is returned (in strict mode)
- Tokens stream as they are generated; the response is not held until completion

### 4.2 Model load flow

```
User: maranode model pull llama3.2:3b
                                  │
                                  ▼
CLI sends RPC to daemon via Unix socket
                                  │
                                  ▼
Daemon checks: is this model already in the store?
   │                              │
   Yes ───────────────────────────┤ already pulled, return
                                  │
   No                              │
                                  ▼
Daemon downloads from configured source (typically Hugging Face)
- Only in non-air-gap mode
- Streaming download with progress reporting
- Computes SHA-256 during download
                                  │
                                  ▼
Verify checksum against manifest
                                  │
                                  ▼
Atomically move blob into place
                                  │
                                  ▼
Audit log entry: model.imported
                                  │
                                  ▼
Return success to CLI
```

**Air-gap mode:** Model download is disabled. Models must be imported from local files:
```
maranode model import /path/to/model.gguf --name llama3.2 --tag 3b
```

This is the workflow for high-security deployments: download the model on an internet-connected machine, transfer via removable media, import locally.

---

## 5. Trust Model

This section answers the question: what does Maranode actually guarantee, and what does it not guarantee?

### 5.1 What we guarantee (when configured correctly)

- **No outbound network traffic** to anywhere except explicitly whitelisted destinations.
- **HMAC-chained audit logs** - modifications to history are detectable by anyone with the HMAC key.
- **Model integrity** - a loaded model matches its declared checksum, or the load fails.
- **No telemetry, ever** - the codebase contains no analytics, no usage reporting, no "anonymous" beacons.

### 5.2 What we depend on

- The Linux kernel doing what it says (iptables rules being enforced).
- The hardware doing what it says (no hidden network interfaces, no compromised firmware).
- The operator's password / disk encryption / physical security.
- The integrity of the binary the operator installed (we provide signed releases).

### 5.3 What we do not guarantee

- Protection against an attacker with root privileges on the host (any local software has this limit).
- Protection against side-channel attacks (timing, power analysis, etc.).
- Protection against the model itself behaving maliciously (we do not verify model behavior, only model bytes).
- Protection against the user copying data out manually (we are not a DLP solution).

### 5.4 The honest disclaimer

Maranode substantially reduces the configuration burden that makes local AI deployments fail. It does not eliminate the need for operators to understand what they are running. We can prove network isolation, but we cannot prove organizational security.

---

## 6. Threat Model

The threats we explicitly design against, ranked by likelihood:

### 6.1 Accidental exfiltration via misconfiguration
**Threat:** An operator follows a blog post that says "just disable telemetry" and assumes that is sufficient. A library deep in the stack still calls home.

**Mitigation:** Kernel-level egress block by default. Even if some embedded library tries to phone home, the packet does not leave the machine. The user can verify with `iptables -L` and `tcpdump`.

### 6.2 Compromise of a single user account
**Threat:** Attacker gains access to a user account and tries to extract sensitive prompts from the audit log.

**Mitigation:** Audit log file mode 0600, owned by the maranode daemon user. Prompts are hashed, not stored verbatim, unless content logging is explicitly enabled. Phase 2 adds RBAC for multi-user deployments.

### 6.3 Audit log tampering
**Threat:** An operator wants to hide that a particular inference happened.

**Mitigation:** HMAC chain detects modifications. Cannot prevent log destruction by root, but a missing log is itself suspicious (an auditor can confirm the system was running but the log is empty).

### 6.4 Supply chain attack
**Threat:** A malicious dependency in the Rust build is shipped to users.

**Mitigation:** Reproducible builds (Phase 1.3), minimal dependency tree, signed releases. We pin dependency versions and audit additions.

### 6.5 Model substitution
**Threat:** An attacker replaces a model file on disk with a poisoned version.

**Mitigation:** SHA-256 verification on every load. If the checksum does not match the recorded value, the load fails and an audit entry is written.

### 6.6 Network-based attack on the API
**Threat:** An attacker reaches the API port and runs unauthorized inferences.

**Mitigation in Core:** Default bind to 127.0.0.1 only. Network exposure requires explicit configuration. Operators should put a reverse proxy with auth in front.

**Mitigation in Enterprise:** Built-in auth, SSO, RBAC.

### 6.7 RAG store poisoning

**Threat:** A malicious or negligent user uploads false, misleading, or
confidential documents into the shared RAG store, causing the model to generate
incorrect or harmful answers for all future queries that retrieve those chunks
(indirect prompt injection).

**Mitigations:**

- **Ingest policy** (`rag.ingest_policy = "admin_only"` or `"allowlist"`):
  Only authorized principals can write to the persistent store.
  Regular users can still extract file text for their own conversation
  (`/v1/rag/extract`) but cannot affect shared retrieval.
- **Audit trail:** Every ingest is logged with the source label, actor
  identity, timestamp, and chunk count. A poisoning incident is detectable and
  traceable even without real-time monitoring.
- **Collection isolation:** Untrusted or user-submitted documents can be kept
  in a separate collection. Queries from general users are routed to the curated
  collection; the unvetted collection is reserved for review.
- **Similarity threshold** (`rag.min_score`): Raising this setting reduces how
  easily a subtly wrong document can dominate retrieval - low-quality chunks
  that score below the threshold are silently dropped.
- **Model-level instruction:** The system prompt instructs the model to cite
  sources explicitly and say "this information is not in the provided documents"
  when uncertain. This makes poisoned answers visible rather than confidently
  wrong.

**What these mitigations do not cover:**
- A compromised admin key. The admin key must be treated as a high-value secret.
- A legitimate, authorized user who uploads incorrect information in good faith.
  Document review workflows (outside Maranode) address this.

### 6.8 Out of scope
- Nation-state adversaries with hardware backdoors
- Compromised CPU/NPU firmware
- Physical extraction of memory contents
- Coercion of legitimate users

These exist; they require different solutions (TEMPEST shielding, hardware HSMs, legal protections). We do not pretend to solve them.

---

## 7. Performance Targets

These are design targets, not measured results. Validation happens in Phase 0 exit criteria.

| Metric | Target | Notes |
|--------|--------|-------|
| First-token latency, 3B model, CPU | < 100ms | Modern x86_64 |
| First-token latency, 3B model, NPU | < 50ms | Intel Core Ultra |
| Throughput, 3B model, CPU | 20 tokens/sec | Q4_K_M quantization |
| Throughput, 3B model, NPU | 50 tokens/sec | Q4_K_M quantization |
| Daemon memory overhead | < 50 MB | Excluding loaded models |
| Audit log write latency | < 5ms | Synchronous fsync |
| Cold start to ready | < 2 seconds | Daemon process startup |

If we cannot hit these on commodity hardware, the value proposition weakens.

---

## 8. Compatibility Surface

### 8.1 API compatibility

Maranode exposes an OpenAI-compatible API at `/v1/*`. This is not aspirational - it is the primary interface. Specifically:

- `POST /v1/chat/completions` with streaming and non-streaming
- `POST /v1/completions` (legacy)
- `POST /v1/embeddings`
- `GET /v1/models`

The goal is that existing OpenAI SDK code (`from openai import OpenAI`) works with no changes other than the `base_url`.

**Maranode-specific extensions (optional, ignored by standard clients):**

- `POST /v1/chat/completions` accepts an optional `rag` object to ground the
  answer in retrieved documents; a grounded response adds a `sources` array.
- `POST /v1/rag/documents`, `GET /v1/rag/collections`, `POST /v1/rag/search`
  manage the optional RAG store. They return `501 Not Implemented` when RAG is
  disabled, so a client can cleanly detect the feature is off.

### 8.2 Model format

GGUF only. We do not support PyTorch checkpoints, SafeTensors, or other formats directly. Operators convert to GGUF using standard tools.

### 8.3 OS compatibility

**Tier 1 (CI tested, supported):**
- Ubuntu 22.04 LTS, 24.04 LTS
- Debian 12
- RHEL 9 / Rocky Linux 9 / AlmaLinux 9
- Alpine Linux 3.19+
- Fedora 39+

**Tier 2 (community supported):**
- Arch Linux
- openSUSE Tumbleweed
- NixOS

**Development only:**
- macOS (Apple Silicon and Intel) - for developer convenience; not the production target

**Not supported:**
- Windows
- Any Linux with kernel < 5.15

---

## 9. Project Structure

The repository is organized to make the architecture visible in the directory tree:

```
maranode/
├── README.md
├── ROADMAP.md
├── ARCHITECTURE.md (this file)
├── LICENSE
├── Cargo.toml                    # Workspace root
├── crates/
│   ├── maranode-daemon/            # The maranoded binary
│   ├── maranode-cli/               # The maranode CLI
│   ├── maranode-api/               # HTTP API layer
│   ├── maranode-inference/         # llama.cpp FFI wrapper
│   ├── maranode-store/             # Model store
│   ├── maranode-rag/               # Optional RAG (vector store, retrieval)
│   ├── maranode-audit/             # Audit log
│   ├── maranode-isolation/         # iptables, namespace management
│   └── maranode-common/            # Shared types
├── docs/
│   ├── install.md
│   ├── usage.md
│   ├── threat-model.md
│   └── verification.md
├── scripts/
│   ├── install.sh                # The curl | sh installer
│   └── build-release.sh          # Reproducible build script
└── tests/
    ├── integration/
    └── e2e/
```

**Why a Rust workspace:** Each crate has a clean boundary and can be tested in isolation. The audit and isolation crates can be reviewed independently by security researchers without needing to understand the inference layer.

---

## 10. Open Questions

Things still open. Some Phase 0 questions have been resolved; these remain.

- **Plugin/extension model:** WebAssembly sandbox or process isolation? This affects whether we can ever support third-party code without breaking the trust model.
- **Multi-device inference:** Can a single inference request be split across multiple devices (e.g., CPU + GPU)? llama.cpp has some support for this; the orchestration layer does not yet use it.
- **Tauri desktop app:** A native desktop wrapper (Tauri) for the web UI would improve the development experience on macOS. Undecided whether this is worth the added packaging complexity.

Resolved:

- **Web UI technology:** Browser-based, served by the daemon. Ships with the binary via rust-embed.
- **Configuration format:** TOML primary, with environment variable overrides for all settings. See `docs/config.toml.example`.
- **Hot reload:** Implemented. `SIGHUP` or `POST /v1/admin/config/reload` applies most settings without restart. See `docs/usage.md`.
- **Model download source:** Hugging Face by default. Operators can pass a full URL or use `model import` for air-gapped installations.

These are listed openly because the architecture should not pretend to be decided where it is not.

---

## 11. References and Prior Art

The design draws explicitly from:

- **Docker** for content-addressed blob storage
- **Ollama** for the model identification convention and CLI ergonomics
- **HashiCorp Vault** for the enterprise vs core split discipline
- **Cosign / Sigstore** for the signing infrastructure model
- **llama.cpp** as the inference engine
- **systemd-journald** for the append-only log philosophy (though we do not use journald itself)

We are not the first project to think about these problems. We are trying to combine known-good ideas into a product that does not yet exist in this combination.

---

**Last updated:** 2026-06  
Draft; updated as the code changes.
