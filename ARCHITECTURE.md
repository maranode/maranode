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

Maranode is structured as five layers, each with a defined responsibility and a defined trust boundary:

```
┌──────────────────────────────────────────────────────────────┐
│                                                              │
│   Layer 4: Interface                                         │
│   • CLI binary (maranode)                                    │
│   • Web UI (served by daemon at /ui)                         │
│   • HTTP API (OpenAI-compatible)                             │
│   • Standalone verifier (maranode-verify)                    │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 3: Orchestration                                     │
│   • Request router & inference queue                         │
│   • Model lifecycle manager                                  │
│   • Audit logger                                             │
│   • Workspace manager                                        │
│   • Identity & auth (local users, OIDC, SAML, API keys)      │
│   • Data classification engine                               │
│   • Incident response & legal hold                           │
│   • Proof / receipt generation                               │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 2: Inference                                         │
│   • llama.cpp via FFI                                        │
│   • Device backends (CPU, CUDA, Metal, ROCm, Vulkan)         │
│   • Model store (content-addressed)                          │
│   • RAG engine (optional)                                    │
│                                                              │
├──────────────────────────────────────────────────────────────┤
│                                                              │
│   Layer 1: Isolation                                         │
│   • iptables egress rules                                    │
│   • Linux network namespaces (per-workspace, lifecycle done; │
│     routing enforcement in progress)                         │
│   • TPM 2.0 PCR read and key sealing                         │
│   • TEE detection (TDX / SEV-SNP)                            │
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

**Why this layering matters:** Each layer can be replaced or omitted without affecting the others. An organization that already has a hardened OS uses only layers 1–4. Layer 1 can be disabled if the operator wants to use their own egress controls. The Phase 3 appliance will replace Layer 0 with a minimal Linux distribution.

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
- Enforce workspace quotas and auth policies

**Technology choices:**
- **Rust** for memory safety in a process that handles untrusted input and crypto
- **Tokio** for async runtime — battle-tested, low overhead
- **Axum** for HTTP (simple, fast, well-supported)
- **rusqlite** for the metadata database (no separate DB process)

**What it does not do:**
- It does not directly load model weights into its own memory — that is delegated to llama.cpp via FFI
- It does not implement TLS termination natively today — operators put a reverse proxy in front (native TLS is planned)

### 3.2 The Inference Engine

llama.cpp, integrated via Rust FFI bindings (`maranode-inference` crate). We use llama.cpp because:
- It is the most mature local inference engine
- It supports GGUF, the de facto standard model format
- It has the broadest hardware support (CPU SIMD, CUDA, ROCm, Metal, Vulkan, OpenVINO)
- Replacing it later, if needed, is a contained refactor

**Boundary:** llama.cpp runs in the same process as the daemon. Model weights are memory-mapped, not copied, so multiple loaded models share memory pages efficiently.

**Device backend status:**
```
CPU (x86_64, aarch64)   — shipped, always available
CUDA (NVIDIA)           — shipped, make build-cuda
Apple Metal             — shipped, make build-metal
AMD ROCm                — shipped, make build-rocm
Vulkan                  — shipped, make build-vulkan
OpenVINO (Intel NPU)    — scaffolding only, inference path not wired up
AMD Ryzen AI / XDNA     — scaffolding only, inference path not wired up
```

Priority order at runtime:
```
1. NPU (if present and model is supported)
2. GPU (if present and model is large enough to benefit)
3. CPU (always available, fallback)
```

### 3.3 The Model Store

Content-addressed storage on the local filesystem, modeled on Docker's image storage but simpler. Lives in `maranode-store`.

**Layout:**
```
/var/lib/maranode/
├── models.db                    # SQLite metadata (models, users, workspaces)
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

Append-only JSON Lines with HMAC chaining. The most security-critical component besides the firewall. Lives in `maranode-audit`.

**Each entry has the structure:**
```json
{
  "ts": "2026-06-13T14:23:45.123Z",
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
- The audit HMAC key can be sealed to a TPM PCR (see section 3.10)

**What this proves:**
- A tampered entry breaks the chain (detectable)
- Deleted entries break the sequence (detectable)
- Cannot prevent an attacker with root from rewriting the entire log, but cannot do so undetectably from a previous known-good snapshot

**What prompts are logged:** SHA-256 of the prompt, not the prompt itself. We do not write user content to disk by default. Operators who want full content logging enable it explicitly with a separate flag.

**SIEM forwarding:** `maranode audit forward <host:port>` streams events to an external SIEM over TCP. Pre-built integrations for Splunk, Elastic, Microsoft Sentinel, and QRadar live in `siem/`.

### 3.5 The Isolation Layer

The point where Maranode differs most from Ollama. Lives in `maranode-isolation`.

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
- Verification uses standard Linux tools (`iptables-save`, `ss`, `ip route`) so the user can re-run them independently
- The daemon re-probes its own egress on a configurable interval; drift causes a `fail-closed` event in the audit log

**Per-workspace network namespaces:** The lifecycle (create/delete/exist) is implemented. Routing inference requests through the namespace is not yet done — this is the next enforcement step.

**What this does not protect against:**
- A malicious operator with root privileges (any local AI tool has this problem)
- Hardware-level exfiltration (out of scope; requires trusted hardware)
- Side-channel attacks on shared infrastructure

### 3.6 Retrieval-Augmented Generation (optional)

A small local model has no reliable knowledge of an organization's private documents and will confidently hallucinate when asked. RAG is the answer: rather than asking the model to recall facts from its weights, Maranode retrieves the relevant source text at query time and instructs the model to answer **only** from that text, with citations. Lives in `maranode-rag`.

**Properties:**
- **Boring storage.** A single SQLite file (`rag.db`) next to the model store holds collections, documents, chunks, and embeddings (little-endian `f32` blobs). No external vector database.
- **Air-gap friendly.** Embeddings are produced by the local inference engine via `/v1/embeddings`. Nothing leaves the host.
- **Exact retrieval.** Brute-force cosine scan over a collection's chunks. Fast at Maranode's target per-tenant volumes; the vector store is the single place to swap in an ANN index later.
- **Auditable and deletable.** Every ingest and retrieval emits an audit event. Deleting a collection removes all its data, satisfying data-erasure obligations.
- **Honest refusal.** When no chunk clears the similarity threshold and grounding is required, the runtime returns "This information is not in the provided documents." instead of guessing.
- **Permission-gated writes, open reads.** Any user may extract text from a file for inline chat context (`POST /v1/rag/extract`); nothing is stored. Permanent writes are governed by `rag.ingest_policy`.
- **Encrypted at rest.** In a workspace, chunk text and summaries are encrypted under the workspace's data-encryption key (DEK). Crypto-shredding the workspace key makes the RAG data unreadable without deletion.
- **Source binding.** When RAG grounds an answer, the signed inference receipt records which document chunks were retrieved, so the grounding is verifiable independently of the response text.

**Ingest policy:**

| Mode | Who can ingest | Notes |
|---|---|---|
| `anyone` | No key required | Default. Suitable for single-user deployments. |
| `admin_only` | Admin key only | Recommended for multi-user. |
| `allowlist` | Admin + listed keys | For service accounts (e.g. a document pipeline). |

**Why RAG and not fine-tuning for facts:** Fine-tuning bakes data permanently into weights (a privacy and erasure problem), must be redone when data changes, and still hallucinates. RAG keeps knowledge in a database that is current, scoped, auditable, and removable.

### 3.7 Proof-Carrying Inference (Receipts)

Every inference produces a signed receipt — a compact, offline-verifiable proof that this node, running this exact model, produced these exact tokens, at this time. Lives in `maranode-common` (receipt types) and is written by `maranode-audit`.

**Receipt contents:**
- Request ID, model ID, model SHA-256
- Prompt SHA-256 (not the prompt itself)
- Response SHA-256
- Token counts and timing
- Environment fingerprint (CPU, OS, kernel, llama.cpp version)
- TPM PCR values (if TPM is present)
- RAG source hashes (if retrieval was used)
- Ed25519 signature over all of the above

**Verification:** `maranode-verify` is a separate standalone binary with no runtime dependencies. It verifies a receipt against a public key without needing a running daemon. The signature scheme is plain Ed25519 over a canonical JSON representation, so it can also be verified by hand.

**Deterministic mode:** With `"deterministic": true` in the request, the runtime pins temperature=0, top_k=1, and seed=0. Combined with the environment fingerprint, this enables replay: `maranode audit replay <request_id>` re-runs the inference under identical conditions and checks that the output matches.

### 3.8 Identity and Authentication

Authentication is built into Core; richer enterprise features (SSO, LDAP group sync, RBAC) are layered on top.

**Identity providers (all implemented):**
- **Local user accounts** — username/password, stored hashed in SQLite. `maranode users create/list/set-password`.
- **API keys** — per-workspace bearer tokens. Identity is asserted by the key; no session cookie.
- **OIDC** — `GET /v1/auth/oidc/login` → provider → callback. Supports any OIDC-compliant provider.
- **SAML 2.0** — SP-initiated SSO. Basic implementation; IdP-initiated and assertion encryption are not yet done.
- **LDAP / Active Directory** — login works; group membership sync is not yet implemented.

**Session model:** `POST /v1/auth/login` returns a session token. `GET /v1/sessions` lists active sessions. `DELETE /v1/sessions/:id` revokes a session. Per-IP rate limiting is applied to all auth endpoints.

**What is not yet built:** Fine-grained RBAC (roles beyond "admin / workspace key holder") is planned but not implemented.

### 3.9 Workspaces

Workspaces are isolated tenants inside one daemon. Each workspace has its own audit segment, model allowlist, rate limit, system prompt, resource quotas, and optionally its own network namespace.

**Properties:**
- Identified by a URL-safe slug (e.g., `clinic-a`)
- Protected by an API bearer key (auto-generated or operator-supplied), or open
- Per-workspace resource limits: `max_concurrent_requests`, `max_models`, `max_memory_bytes`
- Per-workspace system prompt overrides the global default
- Per-workspace RAG collection and encryption key
- **Crypto-shred:** deleting a workspace deletes its DEK; all encrypted data (RAG chunks, logs) becomes unreadable without further deletion

**Management:** `GET/POST /v1/workspaces`, `GET/PUT/DELETE /v1/workspaces/:slug`. Requires the admin key.

### 3.10 TPM and Attestation

Hardware-backed trust lives in `maranode-attestation`.

**What is implemented:**
- **TPM 2.0 PCR read** — reads Platform Configuration Registers at startup via direct `/dev/tpm0` I/O (no tpm2-tools dependency)
- **Binary self-hash** — the daemon hashes its own executable at startup and records it in the audit log
- **Key sealing to PCR policy** — `maranode tpm seal <purpose>` seals a key blob to a TPM PCR state. The key is only unsealable if the PCR values match at unseal time (i.e., the binary has not been tampered with)
- **Attestation report** — `maranode verify attest` builds a JSON report containing binary hash, PCR values, audit chain status, and TEE presence. Signed with the node key.
- **TEE detection** — detects Intel TDX and AMD SEV-SNP. TEE attestation is incorporated into the audit chain. `maranode tpm tee-keygen` generates a key pair bound to the TEE measurement.
- **Key rotation** — `maranode tpm rotate <purpose>` re-seals to the current PCR state and logs the rotation event

**Recovery:** `maranode tpm export-recovery` writes an encrypted recovery bundle for the case where the TPM is replaced or fails.

### 3.11 Data Classification

A policy engine that assigns sensitivity labels to RAG collections and enforces them at ingest and retrieval time. Lives in `maranode-common` (types) and `maranode-api` (enforcement).

**Labels:** `Public < Internal < Confidential < Restricted` (configurable).

**How it works:**
- Each RAG collection can be assigned a label via `PUT /v1/classification/policy`
- At inference time, if a workspace's clearance is below the collection's label, retrieval is blocked
- Violations are written to the audit log as `DataClassificationViolation` events
- DLP label sync: `maranode dlp sync --provider <p>` pulls labels from an external DLP system

### 3.12 Incident Response

A first-class workflow for declaring, investigating, and closing a security incident. Lives in `maranode-api`.

- **Declare:** `maranode incident declare` immediately ends all active sessions, freezes the audit log cryptographically, and opens an incident record.
- **Investigate:** `maranode incident investigate` creates a forensic snapshot of current runtime state.
- **Legal hold:** active incidents prevent retention policies from pruning relevant audit entries.
- **Break-glass credentials:** `maranode incident bg-generate` creates time-limited credentials for emergency access. Each use is logged.
- **Close:** `maranode incident close` restores normal operation and appends a close event to the audit chain.

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
3. Auth check (workspace key / session token / open)
                                  │
                                  ▼
4. Request validated, normalized (OpenAI → internal format)
                                  │
                                  ▼
5. Data classification check (if RAG collection has a label)
                                  │
                                  ▼
6. Audit log entry: inference.start
                                  │
                              RAG enabled?
                             /           \
                           Yes            No
                            │              │
                            ▼              │
                 Embed query with          │
                 local model               │
                            │              │
                            ▼              │
                 Cosine retrieval          │
                 over collection           │
                            │              │
                            ▼              │
                 Inject chunks +           │
                 citations into prompt     │
                            │              │
                            └──────────────┤
                                           │
                                           ▼
7. Model manager: is the model loaded? If not, load it.
                                           │
                                           ▼
8. Scheduler: select device backend (GPU/CPU)
                                           │
                                           ▼
9. llama.cpp generates tokens, streamed back through daemon
                                           │
                                           ▼
10. Sign receipt (Ed25519 over request + response + model hash)
                                           │
                                           ▼
11. Audit log entry: inference.complete (with receipt hash)
                                           │
                                           ▼
12. Response returned to client (SSE stream or single JSON)
```

**Notable properties:**
- No external network calls at any point after model is loaded
- Audit entries written synchronously before response is returned (in strict mode)
- Receipt is always generated; returned to caller only if `with_receipt: true` is set

### 4.2 Model load flow

```
User: maranode model pull llama3.2:3b
                                  │
                                  ▼
CLI sends RPC to daemon via Unix socket
                                  │
                                  ▼
Daemon checks: is this model already in the store?
   │
   Yes → return immediately
   │
   No → download from Hugging Face (whitelist mode only)
         streaming download, SHA-256 computed during download
                                  │
                                  ▼
Verify checksum against manifest
                                  │
                                  ▼
Atomically move blob into place
                                  │
                                  ▼
Audit log entry: model.imported
```

**Air-gap mode:** Model download is disabled. Models must be imported from local files: `maranode model import /path/to/model.gguf --name llama3.2 --tag 3b`. This is the workflow for high-security deployments: download on an internet-connected machine, transfer via removable media, import locally.

---

## 5. Trust Model

### 5.1 What we guarantee (when configured correctly)

- **No outbound network traffic** to anywhere except explicitly whitelisted destinations.
- **HMAC-chained audit logs** — modifications to history are detectable by anyone with the HMAC key.
- **Model integrity** — a loaded model matches its declared checksum, or the load fails.
- **Signed inference receipts** — every inference is signed with an Ed25519 key; the signature is verifiable offline by a third party with no Maranode installation.
- **No telemetry, ever** — the codebase contains no analytics, no usage reporting, no "anonymous" beacons.

### 5.2 What we depend on

- The Linux kernel enforcing iptables rules.
- The hardware doing what it says (no hidden network interfaces, no compromised firmware).
- The operator's password / disk encryption / physical security.
- The integrity of the binary the operator installed. Signed releases are planned (cosign); until that ships, operators should verify the SHA-256 of the binary against the published hash.

### 5.3 What we do not guarantee

- Protection against an attacker with root privileges on the host (any local software has this limit).
- Protection against side-channel attacks (timing, power analysis, etc.).
- Protection against the model itself behaving maliciously (we verify model bytes, not model behavior).
- Protection against the user copying data out manually (we are not a DLP solution).

### 5.4 The honest disclaimer

Maranode substantially reduces the configuration burden that makes local AI deployments fail. It does not eliminate the need for operators to understand what they are running. We can prove network isolation and inference provenance, but we cannot prove organizational security.

---

## 6. Threat Model

### 6.1 Accidental exfiltration via misconfiguration
**Threat:** An operator follows a blog post that says "just disable telemetry" and assumes that is sufficient. A library deep in the stack still calls home.

**Mitigation:** Kernel-level egress block by default. Even if some embedded library tries to phone home, the packet does not leave the machine. Verifiable with `iptables -L` and `tcpdump`.

### 6.2 Compromise of a single user account
**Threat:** Attacker gains access to a user account and tries to extract sensitive prompts from the audit log.

**Mitigation:** Audit log file mode 0600, owned by the maranode daemon user. Prompts are hashed, not stored verbatim, unless content logging is explicitly enabled. RBAC for fine-grained multi-user access control is planned.

### 6.3 Audit log tampering
**Threat:** An operator wants to hide that a particular inference happened.

**Mitigation:** HMAC chain detects modifications. TPM PCR key sealing means the HMAC key is bound to the binary — replacing the binary to forge entries breaks the PCR seal. Cannot prevent log destruction by root, but a missing log is itself suspicious.

### 6.4 Supply chain attack
**Threat:** A malicious dependency in the Rust build is shipped to users.

**Mitigation:** Reproducible builds (script in `scripts/`; independent verification pending), minimal dependency tree, signed releases (in progress with cosign). Dependency versions are pinned and additions are audited.

### 6.5 Model substitution
**Threat:** An attacker replaces a model file on disk with a poisoned version.

**Mitigation:** SHA-256 verification on every load. If the checksum does not match the recorded value, the load fails and an audit entry is written.

### 6.6 Network-based attack on the API
**Threat:** An attacker reaches the API port and runs unauthorized inferences.

**Mitigation (Core):** Default bind to `127.0.0.1` only. Network exposure requires explicit configuration. Workspace API keys enforce per-tenant access.

**Mitigation (Enterprise):** Built-in auth, SSO, RBAC (RBAC not yet built).

### 6.7 RAG store poisoning
**Threat:** A malicious or negligent user uploads false or confidential documents, causing the model to generate incorrect or harmful answers for all future queries.

**Mitigations:**
- **Ingest policy** (`admin_only` or `allowlist`): only authorized principals can write to the persistent store.
- **Audit trail:** every ingest logs the source label, actor, timestamp, and chunk count.
- **Collection isolation:** untrusted documents can be kept in a separate collection from curated knowledge.
- **Similarity threshold** (`rag.min_score`): low-quality chunks that score below the threshold are dropped.
- **Source binding in receipts:** retrieved chunks are recorded in the signed receipt, making poisoned answers traceable.

**What these do not cover:** a compromised admin key, or a legitimate authorized user uploading incorrect information in good faith. Document review workflows outside Maranode address the latter.

### 6.8 Out of scope
- Nation-state adversaries with hardware backdoors
- Compromised CPU/NPU firmware
- Physical extraction of memory contents
- Coercion of legitimate users

These exist; they require different solutions (TEMPEST shielding, hardware HSMs, legal protections). We do not pretend to solve them.

---

## 7. Performance Targets

Measured on commodity hardware with Q4_K_M quantization unless noted.

| Metric | Target | Notes |
|--------|--------|-------|
| First-token latency, 3B model, CPU | < 100ms | Modern x86_64 |
| First-token latency, 3B model, GPU | < 50ms | Mid-range CUDA GPU |
| Throughput, 3B model, CPU | 20 tokens/sec | Q4_K_M quantization |
| Throughput, 3B model, GPU | 50+ tokens/sec | Q4_K_M quantization |
| Daemon memory overhead | < 50 MB | Excluding loaded models |
| Audit log write latency | < 5ms | Synchronous fsync |
| Cold start to ready | < 2 seconds | Daemon process startup |

NPU targets (OpenVINO, AMD XDNA) will be added once those backends are wired up and benchmarked on real hardware.

---

## 8. Compatibility Surface

### 8.1 API compatibility

Maranode exposes an OpenAI-compatible API at `/v1/*`. The goal is that existing OpenAI SDK code (`from openai import OpenAI`) works with no changes other than the `base_url`.

**Standard endpoints:**
- `POST /v1/chat/completions` (streaming and non-streaming)
- `POST /v1/completions` (legacy)
- `POST /v1/embeddings`
- `GET /v1/models`

**Maranode-specific extensions (ignored by standard clients):**
- `rag` object in chat completions for grounded answers; response adds `sources` array
- `with_receipt: true` to receive the signed proof receipt inline
- `deterministic: true` to pin temperature=0, top_k=1, seed=0
- `/v1/rag/*` — document ingestion, collection management, search
- `/v1/workspaces/*` — workspace CRUD (admin key required)
- `/v1/auth/*` — login, OIDC, SAML, session management
- `/v1/audit/*` — log verification, export, compliance bundles
- `/v1/attestation/*` — TPM and TEE attestation endpoints
- `/v1/classification/*` — data classification policy
- `/v1/incident/*` — incident response lifecycle

RAG endpoints return `501 Not Implemented` when RAG is disabled, so a client can cleanly detect the feature is off.

### 8.2 Model format

GGUF only. We do not support PyTorch checkpoints, SafeTensors, or other formats directly. Operators convert to GGUF using standard tools before importing.

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
- macOS (Apple Silicon and Intel) — for developer convenience; not the production target

**Not supported:**
- Windows
- Any Linux with kernel < 5.15

---

## 9. Project Structure

```
maranode/
├── Cargo.toml                        # Workspace root
├── crates/
│   ├── maranode-daemon/              # maranoded binary: startup, config, lifecycle
│   ├── maranode-cli/                 # maranode CLI (model, audit, tpm, incident, …)
│   ├── maranode-api/                 # HTTP API layer, all routes
│   ├── maranode-inference/           # llama.cpp FFI wrapper, device backends, queue
│   ├── maranode-store/               # Model store, user DB, workspace DB, KEK
│   ├── maranode-rag/                 # RAG engine: chunking, embedding, retrieval, crypto
│   ├── maranode-audit/               # Audit log: chain, export, bundle, SIEM forward
│   ├── maranode-attestation/         # TPM PCR, key sealing, TEE detection, attestation report
│   ├── maranode-isolation/           # iptables rules, network namespace lifecycle, probes
│   ├── maranode-common/              # Shared types: receipts, events, classification,
│   │                                 #   workspace, user, incident, approval, baseline
│   ├── maranode-verifier/            # Standalone offline receipt verifier binary
│   └── maranode-bench/               # Benchmark tool (tokens/sec, latency, device compare)
├── docs/
│   ├── install.md
│   ├── usage.md
│   ├── config.toml.example
│   ├── threat-model.md
│   ├── verification.md
│   ├── workspaces.md
│   ├── users.md
│   ├── receipt.md
│   ├── grounding.md
│   ├── compliance.md
│   ├── erasure.md
│   ├── document-intelligence.md
│   └── reproducible-inference.md
├── siem/
│   ├── splunk/                       # Splunk Technology Add-on
│   ├── elastic/                      # Elastic integration
│   ├── sentinel/                     # Microsoft Sentinel connector
│   └── qradar/                       # IBM QRadar connector
├── scripts/
│   ├── install.sh                    # The curl | sh installer
│   └── build-release.sh              # Reproducible build script
├── packaging/
│   ├── debian/                       # .deb package
│   ├── rpm/                          # .rpm spec
│   ├── arch/                         # PKGBUILD
│   └── homebrew/                     # Homebrew tap formula
├── docker/                           # Supplemental Docker compose files
├── demos/
│   └── proof-test/                   # End-to-end receipt verification demo
├── baselines/                        # Signed behavior baselines
└── tests/
    ├── integration/
    └── e2e/
```

**Why a Rust workspace:** Each crate has a clean boundary and can be tested in isolation. The `maranode-audit` and `maranode-isolation` crates can be reviewed independently by security researchers without needing to understand the inference layer. The `maranode-verifier` binary has minimal dependencies by design — it should be auditable in an afternoon.

---

## 10. Open Questions

Things still open as of June 2026.

- **Linux namespace enforcement:** The namespace lifecycle is done; wiring inference requests through the workspace netns is the missing step. This involves changes to how the daemon spawns llama.cpp contexts, which has implications for process isolation vs. in-process FFI.
- **Plugin/extension model:** WebAssembly sandbox or process isolation? This affects whether we can ever support third-party code without breaking the trust model.
- **Multi-device inference:** Can a single inference request be split across multiple devices (e.g., CPU + GPU)? llama.cpp has some support for this; the orchestration layer does not use it.
- **RBAC design:** The access model needs a proper role schema before we can implement it. Unclear whether to build a custom permission system or adopt an existing model (e.g., ABAC with attribute policies).

Resolved:

- **Web UI technology:** Browser-based, served by the daemon. Ships with the binary via rust-embed.
- **Configuration format:** TOML primary, with environment variable overrides for all settings. See `docs/config.toml.example`.
- **Hot reload:** Implemented. `SIGHUP` or `POST /v1/admin/config/reload` applies most settings without restart.
- **Model download source:** Hugging Face by default. Operators can pass a full URL or use `model import` for air-gapped installations.
- **Audit key protection:** Sealed to TPM PCR via `maranode tpm seal audit-hmac`.
- **Tauri desktop app:** Decided against. Browser-based UI shipped with the binary is sufficient; adding a native wrapper adds packaging complexity for minimal gain.

---

## 11. References and Prior Art

The design draws explicitly from:

- **Docker** for content-addressed blob storage
- **Ollama** for the model identification convention and CLI ergonomics
- **HashiCorp Vault** for the enterprise vs core split discipline and the audit log design
- **Cosign / Sigstore** for the signing infrastructure model (releases not yet signed; in progress)
- **llama.cpp** as the inference engine
- **systemd-journald** for the append-only log philosophy (though we do not use journald itself)
- **The Update Framework (TUF)** for thinking about supply chain security

We are not the first project to think about these problems. We are trying to combine known-good ideas into a product that does not yet exist in this combination.

---

**Last updated:** June 2026
