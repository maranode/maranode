# Maranode — Feature Handbook

This is the full handbook of every feature in Maranode. The goal is simple: when
someone ask "do you have X feature?" or "what does X mean?", the answer should be
here, with enough detail to be useful.

Each entry has a short status tag and a description. Where a feature is reachable
from the command line or the HTTP API, the exact command or endpoint is written
next to it, so you can point at it directly.

Status tags:

- **[Done]** — implemented and working in the current tree.
- **[Partial]** — usable but with a known gap, the gap is named in the line.
- **[Planned]** — not built yet, on the roadmap.

This handbook is cross-checked against the source code, the CLI command
definitions, the HTTP routes, the audit event list, and the git history, not only
against the older docs. Where it disagree with `FEATURE-LIST.md` (which is an
older snapshot from early June), this handbook is the newer and correct one: a
large block of features that the snapshot still calls "Planned" are in fact built
now, see the note below.

> **Note on recent work.** A series of commits ("tier 0 finished", "tier 1-2
> done", "tier 3", "incident response", "data classification", "air gapped model
> registry", "doc erasure", "tpm management", receipts and grounding) landed the
> whole Tier 0 to Tier 2 set from the strategy document `FEATURES.md`. So
> proof-carrying inference, reproducible inference, crypto-shredding, grounding
> proof, continuous isolation attestation, behavioral integrity, the approval
> registry, data classification, TPM sealing, confidential computing, incident
> response and legal hold are all implemented today. Only Tier 3 (zero-knowledge
> proofs, federated cross-org verification) stay unbuilt.

---

## Table of contents

1. What Maranode is
2. Inference and model runtime
3. Hardware acceleration
4. Proof-carrying inference (signed receipts)
5. Reproducible inference and replay
6. Network isolation and egress control
7. Continuous isolation attestation
8. Audit log, compliance and evidence
9. SIEM integration
10. Crypto-shredding and right to erasure
11. Legal hold on audit segments
12. Behavioral model integrity (baselines)
13. Air-gapped model registry and approval workflow
14. Data classification and DLP enforcement
15. TPM key sealing and hardware-bound keys
16. Confidential computing (TEE)
17. Incident response and break-glass
18. RAG and document intelligence
19. Grounding proof for RAG
20. Multi-tenant workspaces
21. Users, authentication and SSO
22. API and interfaces
23. Operations and deployment
24. Packaging and distribution
25. Security posture and guarantees
26. Configuration reference
27. Tests, demos and benchmarks
28. What is not built yet
29. Quick lookup: CLI commands
30. Quick lookup: HTTP endpoints

---

## 1. What Maranode is

Maranode is a local LLM runtime for places where a data leak is not acceptable:
hospitals, law firms, banks, government, defense, any regulated environment. It
runs GGUF models with `llama.cpp`, speaks the OpenAI API, and runs on CPU, GPU,
Metal, NPU. That part is table stakes and many tools do it.

The reason Maranode exist is the other part: it can **prove** what it did and it
**fails closed**. Every important event goes into a tamper-evident audit chain.
Egress is blocked by default at daemon start, and you can check this with your own
tools without trusting Maranode. If isolation or the audit log can not be
guaranteed, inference does not run. There is no telemetry and no phone-home.

It ships as two binaries from one codebase: `maranoded` (the daemon) and
`maranode` (the CLI). State is SQLite plus flat files. No Python, no external
database server, no sidecar. A standalone third binary `maranode-verify` exists
only to check receipts offline.

The code is split in 13 Rust crates:

| Crate | Responsibility |
|---|---|
| `maranode-common` | shared types, audit events, receipts, paths, model and workspace types, classification, hold, incident, baseline, approval |
| `maranode-inference` | llama.cpp backend, inference queue, device selection |
| `maranode-store` | GGUF blob store, model manifests, SQLite metadata, user DB, workspace DB, KEK |
| `maranode-audit` | append-only HMAC-chained audit log, bundle, export, retention, signing, verify |
| `maranode-isolation` | iptables egress control, network namespaces, egress probe |
| `maranode-rag` | chunking, local embeddings, vector store, extract (PDF/OCR), at-rest encryption |
| `maranode-attestation` | TPM PCR read, key sealing, PCR policy, rotation, TEE detection, attestation report |
| `maranode-api` | the HTTP server, all routes, OpenAI surface, DLP connectors, incident, legal hold |
| `maranode-daemon` | the daemon process: config, lifecycle, probe loop, hot reload, shutdown, unix socket |
| `maranode-cli` | the `maranode` command line |
| `maranode-verifier` | the standalone `maranode-verify` receipt checker |
| `maranode-bench` | benchmark tool |

---

## 2. Inference and model runtime

**Local text inference** — **[Done]**. Runs GGUF models locally through
`llama.cpp`. No network call for a normal inference. The backend lives in
`maranode-inference` (`LlamaCppEngine`). A `stub` engine also exist for tests and
for builds without the native backend.

**OpenAI-compatible chat completions** — **[Done]**. `POST /v1/chat/completions`.
You change only the `base_url` of any OpenAI SDK and it works. Supports the normal
`messages` array, `model`, `temperature`, `top_k`, `max_tokens`, `seed`.

**Streaming and non-streaming** — **[Done]**. Server-sent events (SSE) for token
streaming, or a single JSON response when `stream` is false.

**Embeddings endpoint** — **[Done]**. `POST /v1/embeddings`, OpenAI shape. Used by
RAG and also callable directly. Needs an embedding-type model in the store.

**Concurrent request queue** — **[Done]**. `InferenceQueue` in
`maranode-inference`. `max_parallel` controls how many requests run at the same
time (each slot keeps its own KV cache), `max_queue_depth` controls how many can
wait before new ones get HTTP 503. Both set in `[inference]` config.

**Content-addressed model store** — **[Done]**. Models are stored by the SHA-256
of their weights. The hash is checked on every load. If two models have identical
weights they are stored once (automatic deduplication). A partial or interrupted
download is never visible as a usable model (atomic write). Lives in
`maranode-store` (`ModelStore`, `blob`).

**Pull model from Hugging Face** — **[Done]**. `maranode model pull
<owner/repo/file.gguf> --name <name> --tag <tag> --quant <q>`. Also accept a full
`https://` URL. Online only, so turn off air-gap first if the daemon runs.

**Import model from local file** — **[Done]**. `maranode model import <path>
--name <name> --tag <tag>`. Works fully offline, this is the path for air-gapped
installs.

**List and remove models** — **[Done]**. `maranode model list`, `maranode model
remove <model>`. Over the API: `GET /v1/models`, `GET /v1/models/details`,
`DELETE /v1/models/:model_id`.

**Models pagination** — **[Done]**. `GET /v1/models` supports pagination so a
large store does not return one huge list.

**Model context window metadata** — **[Done]**. The store keeps the context
window size of each model as metadata and exposes it in the model details.

**Token count estimation** — **[Done]**. The runtime estimates prompt token count
to keep input inside the context window. It is still an estimate, exact
per-tokenizer budgeting is a later improvement.

**Model quantization tools** — **[Done]**. `maranode model quant inspect <file>`
shows the GGUF quantization of a file or stored model. `maranode model quant
recommend` suggests a quantization from parameter count and available RAM.
`maranode model quant list` lists the known quant formats. The quant logic lives
in `maranode-cli/commands/quant.rs`.

**Model cache eviction (LRU)** — **[Done]**. Loaded models live in an in-memory
cache that evicts the least-recently-used model when it grows, instead of keeping
every model resident until restart. `evict_lru` in `maranode-inference/llama.rs`,
from the "Model memory pressure eviction" commit.

**Embedding model type** — **[Done]**. `--type embedding` on pull/import marks a
model as an embedder. RAG needs at least one such model.

**Split one inference across several devices** — **[Planned]**. Today one
inference uses one device class.

---

## 3. Hardware acceleration

Device is chosen automatically at startup, order is Metal > CUDA > ROCm > Vulkan >
OpenVINO > Ryzen AI > CPU. You can force a device with `device =` in config or
`MARANODE_DEVICE`.

**CPU (x86_64 and aarch64)** — **[Done]**. Always available, always compiled in.

**NVIDIA CUDA** — **[Done]**. `make build-cuda`.

**Apple Metal** — **[Done]**. `make build-metal`.

**AMD ROCm** — **[Done]**. `make build-rocm`.

**Vulkan** — **[Done]**. `make build-vulkan`.

**Automatic device selection** — **[Done]**. The `auto` mode picks the best
compiled backend at start. Covered by `device_selection_test`.

**Intel NPU via OpenVINO** — **[Partial]**. Build scaffolding and `Dockerfile.openvino`
are present, but it needs a cmake-flag build and more testing.

**AMD Ryzen AI / XDNA NPU** — **[Partial]**. Same state as OpenVINO, scaffolding
present, not yet fully tested.

---

## 4. Proof-carrying inference (signed receipts)

This is the central differentiator. Every inference can carry its own evidence.

**Signed inference receipt** — **[Done]**. Each inference produces a receipt: a
JSON object that binds model identity (the GGUF SHA-256), quantization, decode
parameters, input hash, output hash, token counts, a timestamp, and the daemon
signing key. It is signed with Ed25519. The format is versioned (`version: 1`).
Defined in `maranode-common/receipt.rs`, full field table in `docs/receipt.md`.

**Ask for the receipt inline** — **[Done]**. Add `"with_receipt": true` to the
chat completion body and the response carries a `receipt` object at top level.

**Receipt always written to audit** — **[Done]**. Even if you did not ask for it
inline, the receipt is written to the audit log as an `inference_receipt` event,
so you can get it later.

**Extract a past receipt** — **[Done]**. `maranode audit prove <request_id>`
prints the signed receipt for an inference. The `request_id` is the
`X-Request-Id` header from the API response.

**Standalone offline verifier** — **[Done]**. A separate binary `maranode-verify`
checks a receipt with no dependency on the daemon or its database. `maranode-verify
receipt.json` does the signature check, `--input prompt.json --output response.txt`
also re-checks the input and output hashes. Exit code 0 is verified, 1 is failed.
Source in crate `maranode-verifier`.

**Verify by hand** — **[Done]**. Because the receipt is plain Ed25519 over
canonical JSON, a third party can verify with Python (`cryptography` or `PyNaCl`)
and never install any Maranode binary. The steps are written out in
`docs/receipt.md`.

**Stable node signing key** — **[Done]**. The signing key is generated on first
start and stays stable across restarts. It is stored at
`<data_dir>/bundle_signing.key`, the public key at `bundle_signing.pub`. You can
also read the public key from the `signing_key_id` field of any receipt, or from
`GET /v1/audit/signing-key`.

**TPM PCR in the receipt** — **[Done]**. The receipt carries an optional
`tpm_pcr` field with a PCR composite quote when a TPM is present, so the receipt
can be tied to the machine state.

**Honest limit** — the receipt proves the node signed these exact bytes. By
itself it does not prove the node was air-gapped at the time, for that you also
check the isolation attestation chain (section 7). This is stated openly in the
docs.

---

## 5. Reproducible inference and replay

**Bit-exact reproducible inference** — **[Done, conditional]**. With greedy
decoding and a deterministic-kernels build, the same input re-runs to the same
output bytes. You enable greedy mode with `"deterministic": true` in the request
(or `--deterministic`), which pins temperature 0, top-k 1, seed 0.

**Deterministic kernels build** — **[Done]**. Compile with `cargo build --features
deterministic-kernels`. This passes `-DGGML_DETERMINISTIC=ON` to the llama.cpp
cmake build and fixes the floating-point reduction order in RMSNorm, MatMul and
attention. The receipt records `env.kernel_build_id` with a `+deterministic`
suffix when this was active, so a verifier can tell offline.

**Environment fingerprint in receipt** — **[Done]**. The receipt records
`env.kernel_build_id`, `env.thread_count`, `env.device_class`. These let a
verifier confirm the hardware class and build before trusting a replay.

**Replay a past decision** — **[Done]**. `maranode audit replay <request_id>`
re-runs the original inference through the running daemon and compares the new
output hash against the stored one. It needs `log_prompts = true` so the original
messages exist to replay.

**Stated conditions** — reproducibility holds for greedy decoding, same model
file, deterministic-kernels build, and same hardware class. Temperature breaks it
by design, x86 vs ARM breaks it, GPU vs CPU breaks it, different quant breaks it.
The full table is in `docs/reproducible-inference.md`. The runtime never claims
unconditional reproducibility. Covered by `repro_ci_test`.

---

## 6. Network isolation and egress control

**Default-deny egress** — **[Done]**. At daemon start the iptables OUTPUT chain
default policy is set to DROP. Loopback and the configured API port are explicitly
allowed. Nothing else leaves the machine. Lives in `maranode-isolation`
(`iptables`, `Isolator`). This is not a config flag you can forget, it is applied
at start.

**Air-gap mode** — **[Done]**. `mode = "airgap"` in `[isolation]` (the default).
All outbound traffic blocked.

**Whitelist mode** — **[Done]**. `mode = "whitelist"` plus
`[[isolation.whitelist]]` blocks for named host/port. Used when you must reach,
for example, a private Hugging Face mirror, without opening everything.

**Disabled mode** — **[Done]**. `mode = "disabled"` or the `--no-isolation` flag.
Development convenience only, marked unsafe in the config.

**Allowed inbound sources** — **[Done]**. `allowed_sources` limits which source
addresses may reach the API port (loopback only by default).

**Verify network yourself** — **[Done]**. `maranode verify network` runs an
active TCP egress probe and also dumps the iptables rules so you can re-run the
check with `iptables -L` / `iptables-save` and `tcpdump` on your own. The point is
you do not have to trust Maranode to confirm the air-gap.

**Detect manual firewall changes** — **[Partial]**. The probe can report on the
current rule state, full detection and alerting on out-of-band rule edits is still
being hardened.

**Per-workspace network namespaces** — **[Partial]**. The lifecycle to give a
workspace its own Linux network namespace is scaffolded in
`maranode-isolation/netns.rs`, but enforcement is not finished yet.

---

## 7. Continuous isolation attestation

**Periodic egress self-probe** — **[Done]**. The daemon re-probes its own egress
posture on an interval (the probe loop in `maranode-daemon/probe.rs`), not only
once at install. Each result is written into the audit chain as an
`isolation_probe` event with the list of probed hosts and whether each was
reachable, plus a hash of the iptables snapshot.

**Fail-closed on drift** — **[Done]**. If isolation can no longer be confirmed,
the runtime refuses inference and records why. This is the opposite of a tool that
keeps serving when the air-gap is uncertain.

**Isolation timeline report** — **[Done]**. `maranode audit isolation-report`
prints the probe timeline from the audit log, with `--from` / `--to` time bounds
and `--only-broken` to show only the moments isolation was broken. So you can show
an auditor that the air-gap held across the whole period, not just at one moment.

**Honest limit** — this proves the OS egress rules were present and a probe could
not get out during each check. It does not defeat a root attacker who tampers with
both the rules and the probe in the same window, nor kernel/hardware exfiltration.
The threat model labels this openly.

---

## 8. Audit log, compliance and evidence

**Append-only audit log** — **[Done]**. Every state-changing event is written as
one JSON line (JSONL) into the audit log. The full event list is in section 8.1
below. Lives in `maranode-audit` (`AuditLog`).

**HMAC chain** — **[Done]**. Each line is linked to the previous one with an HMAC.
If a line is deleted or modified, the chain breaks at that point and the break is
detectable. The HMAC key is the `audit.key` file in the data directory.

**Verify the whole chain** — **[Done]**. `maranode audit verify` checks the entire
chain in one command and reports the first break if any. Returns a `VerifyResult`.
Over the API: `GET /v1/audit/entries`. Covered by `audit_chain_test`.

**View recent events** — **[Done]**. `maranode audit tail` shows the last events.

**Prompts not stored by default** — **[Done]**. The audit log records the SHA-256
of a prompt, not its text. The content is never written to disk unless you opt in
with `log_prompts = true`. A hash is a fingerprint, not the content.

**Opt-in full-content logging** — **[Done]**. With `log_prompts = true` the full
prompt and full response are also stored in the event (and, in a workspace,
encrypted under the workspace key). Off by default.

**Compliance exports** — **[Done]**. `maranode audit export --format <f>` writes a
CSV shaped for a specific regime: `gdpr` (Article 30 record of processing),
`hipaa` (access log), `soc2` (security events), `iso27001` (event log). Supports
`--workspace`, `--from`, `--to`, `--output`. Over the API: `GET /v1/audit/export`.

**Evidence bundle (ZIP)** — **[Done]**. `maranode audit bundle` creates a ZIP with
the log, an integrity report and a manifest. Over the API: `GET /v1/audit/bundle`,
and per workspace `GET /v1/audit/bundle/:workspace`.

**Signed evidence bundles** — **[Done]**. The exported bundle is signed with the
node signing key, so the bundle itself is verifiable and not only the events
inside it. (This was the "Signed evidence bundles added" commit, it closes the
earlier partial state.)

**Retention prune** — **[Done]**. `maranode audit prune --older-than <days>`
removes stale entries. Without the delete flag it only counts what would go. Over
the API: `POST /v1/audit/prune`. The chain stays consistent for what remains.

**Automatic retention enforcement** — **[Done]**. A retention automator can apply
the retention policy on a schedule instead of only on a manual prune command (the
"retention automator added" commit). Logic in `maranode-audit/retention.rs`.

**Audit backup and restore** — **[Done]**. `maranode audit backup` makes a ZIP of
all audit files and keys (`--include-workspaces` to also pull every workspace
audit log). `maranode audit restore <zip>` restores them (`--force` to overwrite).

**Signing key endpoint** — **[Done]**. `GET /v1/audit/signing-key` returns the
node public signing key so a consumer can pin it.

**Audit log rotation** — **[Done]**. The active log is rotated once it passes a
size limit (`logging.audit_max_mb`, default 256) or its oldest entry passes an age
limit (`logging.audit_max_age_days`, off by default). The rotated part is deflate
compressed into a sealed segment under `<data_dir>/audit-rotated/`, listed in a
`segments.json` manifest with the seq range and a SHA-256 of the segment file. The
HMAC chain does not reset: the next active log continues from the segment last
hmac, and a restart with an empty active log recovers the seq from the manifest.
Size rotation runs inline on append; the retention scheduler handles the age
trigger and drops segments older than the retention horizon. `maranode audit
verify` walks every segment and then the active file as one chain; `maranode audit
segments` lists the sealed segments. Logic in `maranode-audit/rotate.rs`.

### 8.1 Every audit event type

These are the event kinds the audit chain can record (from
`maranode-common/events.rs`). Each one answers "is this action logged?" — yes, all
of them are:

`daemon_start`, `daemon_stop`, `isolation_applied`, `model_imported`,
`model_removed`, `inference_start`, `inference_complete`, `inference_failed`,
`inference_receipt`, `rag_document_ingested`, `rag_retrieval`, `isolation_probe`,
`workspace_shredded`, `config_reloaded`, `audit_verified`, `binary_attested`,
`model_baseline_checked`, `model_drift_detected`, `model_approval_granted`,
`model_approval_revoked`, `model_load_blocked`, `data_classification_violation`,
`data_label_assigned`, `dlp_sync_completed`, `tpm_key_sealed`, `tpm_unseal_failed`,
`tpm_key_rotated`, `incident_declared`, `audit_frozen`, `audit_unfrozen`,
`forensic_snapshot`, `break_glass_used`, `incident_phase_changed`,
`incident_resolved`, `legal_hold_placed`, `legal_hold_released`,
`legal_hold_key_generated`, `tee_attested`.

---

## 9. SIEM integration

**Forward audit to a SIEM** — **[Done]**. `maranode audit forward <host:port>`
streams audit events to a SIEM over syslog, CEF format over TCP or UDP (RFC 5424).
Supports `--transport tcp|udp` and `--from` / `--to` time bounds.

**Splunk app** — **[Done]**. A ready Splunk Technology Add-on lives in
`siem/splunk/maranode/` — `props.conf`, `transforms.conf`, `savedsearches.conf`,
`app.conf`. Field extraction and saved searches included.

**Elastic SIEM** — **[Done]**. `siem/elastic/` ships an ingest pipeline, an index
template and a `filebeat.yml` so events land in ECS-style fields.

**Detection content** — **[Done]**. Ready detection rules for several platforms:
`siem/detection/splunk_detections.spl` (Splunk SPL),
`siem/detection/sentinel_kql.kql` (Microsoft Sentinel KQL),
`siem/detection/qradar_aql.aql` (IBM QRadar AQL), plus a shared
`siem/detection/field-schema.md` documenting the field mapping.

The differentiator is not the connector (a Splunk add-on for other tools already
exists), it is that what you feed the SIEM are tamper-evident, signed events, not
best-effort logs.

---

## 10. Crypto-shredding and right to erasure

**Per-workspace data encryption key (DEK)** — **[Done]**. Each workspace has its
own 32-byte DEK. Every RAG chunk, document summary and stored text for that
workspace is encrypted with AES-256-GCM under the DEK before it touches disk.
Logic in `maranode-store/workspace_db.rs` and `maranode-rag/crypt.rs`.

**Master key wrapping (KEK)** — **[Done]**. DEKs are wrapped (encrypted) under a
master key-encryption key stored at `<data-dir>/master.key`. The DEK is never on
disk in clear. `maranode-store/kek.rs` does the wrap/unwrap.

**Crypto-shred a workspace** — **[Done]**. `maranode workspace shred <slug>
--yes` destroys the DEK: the DEK column is set to NULL and the wrapped value is
removed. After that the ciphertext on disk is mathematically unreadable, even with
the master key, because the wrapped DEK no longer exist. This is how an erasure
request is honored without scrubbing every disk page.

**Signed deletion certificate** — **[Done]**. Shredding writes a
`workspace_shredded` event into the HMAC chain. `maranode audit export-cert
<slug>` exports a one-page plain-text deletion certificate (workspace slug,
timestamp, audit sequence number, actor, HMAC, erasure statement). Because the
certificate comes from a chained entry, tampering is detectable with `maranode
audit verify`. Full mapping to GDPR Article 17 is in `docs/erasure.md`.

**Master key rotation** — **[Done]**. `WorkspaceDb::rotate_kek` re-wraps all DEKs
from an old master key to a new one. A CLI wrapper around it is a later
convenience.

**Stated limits** — shredding covers data at rest inside Maranode. It does not
reach plaintext a user already exported, and a backup taken before the shred still
holds the old ciphertext, so backups must be handled separately. Chunk metadata
(document names, page numbers) is stored in clear and is not covered by the DEK.
All of this is written openly in `docs/erasure.md`. Covered by `shred_test`.

---

## 11. Legal hold on audit segments

**Separate compliance key** — **[Done]**. Legal hold uses its own keypair,
separate from the admin key by design, so a compliance officer — not IT — controls
holds. `maranode hold generate-key` creates the keypair (admin runs it once and
hands the private key to compliance). `--seal-tpm` can seal the hold key into the
TPM.

**Place a hold** — **[Done]**. `maranode hold place` puts a hold on a range of
audit entries, with an optional ISO-8601 expiry. A held segment can not be pruned
or modified by anyone, including root, until released. Over the API: `POST
/v1/legal-hold/place`, `POST /v1/legal-hold/generate-key`.

**Release a hold (two-party)** — **[Done]**. Release needs the compliance
signature. `maranode hold sign-release` is run offline by the compliance officer
with their private key, then `maranode hold release` is submitted by the admin
with that signature. So IT alone can not lift a hold. Endpoints: `POST
/v1/legal-hold/sign-release`, `POST /v1/legal-hold/release/:id`.

**List holds** — **[Done]**. `maranode hold list` shows active and released holds.
API: `GET /v1/legal-hold/list`. Implementation in
`maranode-api/legal_hold.rs` and `hold_recovery.rs`, types in
`maranode-common/hold.rs`.

**Honest caveat** — if the compliance officer loses their key, IT can not help.
Key recovery for non-technical key holders is a real operational concern.

---

## 12. Behavioral model integrity (baselines)

**Behavioral baseline** — **[Done]**. A baseline is a set of signed
prompt/expected-output test vectors for a specific model SHA-256. On model load
the runtime can run these vectors and compare the outputs to the baseline. SHA-256
checks the bytes on disk, a baseline checks the *behavior*, which catches a model
whose weights were subtly modified to behave differently on trigger inputs.
Types in `maranode-common/baseline.rs`.

**Create a baseline** — **[Done]**. `maranode baseline create --model-sha <hash>
--model-id <id> --vector "prompt=expected_text" ...`. The expected SHA-256 is
computed for you from each expected text. `--drift-tolerance` sets how many vector
mismatches are tolerated before drift is declared (default 0).

**Sign and verify** — **[Done]**. `maranode baseline sign <file>` signs a baseline
with the local baseline signing key. `maranode baseline verify <file>` checks the
signature and prints a summary.

**Check a model against its baseline** — **[Done]**. `maranode baseline check
--model <name:tag>` runs the vectors against a model through the running daemon. If
omitted, the baseline is looked up at `<data_dir>/baselines/<model_sha256>.mrn-baseline`.
Over the API: `POST /v1/baseline/check`.

**Public baseline registry** — **[Done]**. `maranode baseline fetch <sha-prefix>`
fetches a baseline from the public registry by model hash. A starter registry is
shipped at `baselines/registry.json`.

**Drift events** — **[Done]**. A baseline check writes `model_baseline_checked`,
and if outputs drift, `model_drift_detected`, into the audit chain. The runtime can
refuse to serve a drifted model (fail-closed). Covered by `drift_test`.

---

## 13. Air-gapped model registry and approval workflow

**Approval-gated model loading** — **[Done]**. A model must be registered,
reviewed and approved before the daemon will load it. The daemon refuses any model
without a valid signed approval token. A blocked load writes `model_load_blocked`.
Types in `maranode-common/approval.rs`.

**Submit and approve** — **[Done]**. `maranode registry submit <model-sha>`
submits a model for review. `maranode registry approve <sha256> --expires-days N`
approves it and issues a signed token. `maranode registry revoke <sha256>` revokes
it. List with `maranode registry list` (submissions), `maranode registry tokens`
(issued tokens). Endpoints: `POST /v1/registry/submit`, `GET /v1/registry/pending`,
`POST /v1/registry/approve/:sha256`, `POST /v1/registry/revoke/:sha256`, `GET
/v1/registry/tokens`. Events: `model_approval_granted`, `model_approval_revoked`.

**Air-gapped token transfer** — **[Done]**. `maranode registry export-token` and
`maranode registry import-token` move a signed approval token across an air gap as
a file. `maranode registry verify-token` checks a token file signature and prints
its content offline.

**Approval web UI** — **[Done]**. `maranode registry ui` opens the approval web UI
in the browser, served at `GET /v1/registry/ui`.

**Change-management hooks** — **[Done]**. The registry can call into an external
change-management system. `maranode registry hooks-test` tests connectivity to the
configured systems, endpoint `POST /v1/registry/hooks/test`. Logic in
`maranode-api/changemgmt.rs`.

---

## 14. Data classification and DLP enforcement

**Classification labels** — **[Done]**. Four ordered labels exist:
`PUBLIC`, `CONFIDENTIAL`, `RESTRICTED`, and a PII/PHI level. Each label has a
numeric level so the policy can compare clearances. Defined in
`maranode-common/classification.rs` (`DataLabel`).

**Policy enforcement at inference time** — **[Done]**. A policy maps RAG
collections to a label and gives each workspace a clearance level. When a workspace
tries to read a collection above its clearance, the access is a violation. The
check runs inside the chat and RAG paths, not as an afterthought. A violation
writes `data_classification_violation`, and can block the request when the policy
say so (fail-closed). Assigning a label writes `data_label_assigned`.

**Manage the policy** — **[Done]**. `GET /v1/classification/policy` reads it. `PUT
/v1/classification/collections/:name` sets (or `DELETE` removes) a collection
label and whether to block on violation. `PUT /v1/classification/workspaces/:slug`
sets a workspace clearance.

**DLP label sync** — **[Done]**. `maranode dlp sync --provider <p>` pulls data
labels from an enterprise DLP system into the classification policy, so you do not
re-tag documents only for Maranode. Providers: `purview` (Microsoft Purview, with
Azure tenant/client/secret), `forcepoint` and `symantec` (base URL, username,
password). Endpoint `POST /v1/dlp/sync`, event `dlp_sync_completed`. Connectors in
`maranode-api/dlp/` (`purview.rs`, `forcepoint.rs`, `symantec.rs`). Covered by
`classification_tests`.

---

## 15. TPM key sealing and hardware-bound keys

The attestation crate (`maranode-attestation`) handles TPM and binary integrity.

**TPM 2.0 PCR read** — **[Done]**. On Linux, reads the TPM Platform Configuration
Registers directly from the device, SHA-256 bank. `maranode tpm status` shows TPM
availability, sealed-key status and current PCR values.

**Binary self-hash at start** — **[Done]**. At startup the daemon hashes its own
binary and records it as a `binary_attested` audit event. So the audit log says
which exact build was running.

**Seal keys to PCR policy** — **[Done]**. `maranode tpm seal <purpose>` seals a key
purpose (`workspace-kek`, `audit-hmac`, or `admin-cred`) so the key only
materializes when the machine is running the expected software state. PCR policy
profiles: `server` (PCR 0,7) and `workstation` (PCR 0,7,11), or custom indices.
`maranode tpm capture-pcrs` writes the PCR policy file. Event `tpm_key_sealed`.

**Software fallback** — **[Done]**. When no TPM is present, sealing falls back to a
passphrase-encrypted software key, so the same workflow works on machines without
a TPM. `is_tpm2_tools_available` decides the backend.

**Unseal test and verify** — **[Done]**. `maranode tpm unseal-test <purpose>`
confirms a key can unseal without returning the key material. `maranode tpm
verify-pcrs` checks current PCRs against the saved policy. A failed unseal writes
`tpm_unseal_failed`.

**Recovery bundle** — **[Done]**. `maranode tpm export-recovery` writes an
encrypted bundle of all sealed purposes (for TPM replacement or firmware update),
`maranode tpm import-recovery` re-seals from it. Logic in
`maranode-attestation/rotation.rs`.

**Key rotation and log** — **[Done]**. `maranode tpm rotate <purpose>` re-seals a
key with new PCRs or a new passphrase and records a reason, event `tpm_key_rotated`.
`maranode tpm rotation-log` shows the rotation history.

**Attestation report** — **[Done]**. `maranode verify attest` builds a runtime
integrity report (binary hash, PCR values, audit chain status), `--output` to save
JSON. Over the API: `GET /v1/attestation/report` and `GET
/v1/attestation/public-key`. Remote third party can pull and check it.

---

## 16. Confidential computing (TEE)

**TEE detection** — **[Done]**. The runtime detects a trusted execution
environment (Intel TDX, AMD SEV-SNP) when present. `maranode tpm tee-probe` prints
a TEE report. Over the API: `GET /v1/attestation/tee`. Logic in
`maranode-attestation/tee.rs` (`detect_tee`, `TeeType`, `TeeReport`).

**TEE attestation into the audit chain** — **[Done]**. The TEE report is attested
into the same audit chain as everything else (event `tee_attested`). The
differentiator is not the TEE itself (confidential serving exists elsewhere), it
is that the encrypted-memory guarantee becomes part of the provable record.
Verify endpoint `POST /v1/attestation/tee/verify`.

**TEE API-layer encryption** — **[Done]**. `maranode tpm tee-keygen` generates an
AES-256-GCM key for encrypting prompts/responses at the API layer so the host
operator can not read them. Logic in `maranode-api/tee_encrypt.rs`.

**TEE performance report** — **[Done]**. `GET /v1/attestation/tee/perf` reports
the measured overhead of running in the TEE, since this cost is real and needs to
be shown. `measure_tee_perf` in `maranode-attestation/perf.rs`.

**Hardware caveat** — TDX/SEV-SNP need specific hardware and kernels, and
performance is worse than bare metal. This is a premium path for a subset of
deployments, not the default.

---

## 17. Incident response and break-glass

**Declare an incident** — **[Done]**. `maranode incident declare` immediately ends
active user sessions and freezes the audit log (no pruning until the incident
closes). `--webhook` notifies configured URLs on phase changes. Endpoint `POST
/v1/incident/declare`, event `incident_declared` plus `audit_frozen`. Logic in
`maranode-api/incident.rs`.

**Audit freeze is cryptographic** — **[Done]**. The freeze is enforced, not just a
toggle: while frozen the retention/prune path can not touch the log. Unfreezing on
resolve writes `audit_unfrozen`.

**Incident lifecycle** — **[Done]**. `maranode incident investigate` moves the
incident to the investigating phase (`incident_phase_changed`), `maranode incident
resolve` closes it and unfreezes the log (`incident_resolved`). `maranode incident
status` shows the current state. Endpoints under `/v1/incident/*`.

**Forensic snapshot** — **[Done]**. `maranode incident snapshot` captures the
current runtime state as a forensic snapshot, event `forensic_snapshot`. Logic in
`maranode-api/forensic.rs`. Endpoint `POST /v1/incident/snapshot`.

**Break-glass credentials** — **[Done]**. `maranode incident bg-generate` creates a
single-use credential that bypasses normal auth for an emergency. `maranode
incident bg-use` consumes it and forces a mandatory `break_glass_used` audit event,
so emergency access is always recorded. Endpoints `POST
/v1/incident/break-glass/generate` and `/v1/incident/break-glass/use`.

---

## 18. RAG and document intelligence

**Fully local RAG** — **[Done]**. Retrieval-augmented generation with local
embeddings and a SQLite vector store. No external vector database, nothing leaves
the machine. Enable with `--rag` or `[rag] enabled = true`. Crate `maranode-rag`
(`RagEngine`, `VectorStore`, `Embedder`).

**Brute-force cosine retrieval** — **[Done]**. Exact cosine similarity over all
chunks, no approximate index. Slower at large scale but exact, which matters for a
provable trail. Math in `maranode-rag/math.rs`.

**Cited, grounded answers** — **[Done]**. Answers cite the source chunks they used.
Use from chat with `--rag` and optional `--collection`, or set `with_receipt` to
get the source references in the receipt (see section 19).

**Honest refusal** — **[Done]**. When no chunk passes the `min_score` threshold,
the model is told there is nothing relevant and says so instead of guessing. You
raise `min_score` (e.g. 0.3–0.5) to make refusal meaningful.

**Collections** — **[Done]**. Create, list, delete and manage documents in named
collections. `maranode rag add`, `maranode rag list`, `maranode rag search`. API:
`GET /v1/rag/collections`, `DELETE /v1/rag/collections/:name`, `GET
/v1/rag/collections/:name/documents`, `POST /v1/rag/search`.

**Ingest documents** — **[Done]**. `POST /v1/rag/documents` (and
`/v1/rag/documents/upload`) ingest a document: it is chunked, embedded and stored.
Plain text, Markdown, CSV, log, reStructuredText, common data formats (JSON, YAML,
TOML, HTML, CSS) and source code (Rust, Python, JS/TS, Go, Java, C/C++, C#, and
more) are supported directly. Chunking in `maranode-rag/chunk.rs`, controlled by
`chunk_size`, `chunk_overlap`, `top_k`, `min_score`, `max_context_chars`. Event
`rag_document_ingested`, retrieval logged as `rag_retrieval`.

**PDF text extraction** — **[Done]**. PDF ingest pulls text with page numbers and
document metadata. Extractor in `maranode-rag/extract.rs` (`DocumentContent`,
`Page`, `DocumentMeta`).

**OCR for scanned PDFs** — **[Done]**. Scanned/image PDFs are run through OCR
instead of being rejected (the "ocr for scanned pdf" commit).

**Table extraction from PDFs** — **[Done]**. Tables in PDFs are extracted into
structured Markdown (same commit set).

**Per-document summary** — **[Done]**. Each document can carry a summary. `GET
/v1/rag/documents/:id/summary` reads it, `POST /v1/rag/documents/:id/summarize`
(re)generates it. Summaries are encrypted at rest under the workspace DEK.

**Ephemeral chat attachment** — **[Done]**. `POST /v1/rag/extract` pulls text from
an uploaded file for one conversation and stores nothing permanently. This path is
always open to any user, separate from the ingest permission policy below.

**Encrypted RAG store** — **[Done]**. In a workspace, chunk text and summaries are
encrypted with AES-256-GCM under the workspace DEK before writing (the "encrypt
workspace rag store" commit), which is what makes crypto-shred (section 10) work.

**Ingest permission policy** — **[Done]**. `ingest_policy` controls who may write
to the persistent store: `anyone`, `admin_only`, or `allowlist` (with
`ingest_allowlist` of API keys). The admin key is always allowed. Note the
ephemeral extract path is not governed by this, only permanent ingest is.

**Code-aware chunking** — **[Done]**. Source files are split along symbol
boundaries instead of fixed character windows: brace languages cut on top-level
`{ }` blocks (functions, structs, classes, impls) and Python on `def`/`class`,
with decorators and leading doc comments kept attached. Small units merge up to
`chunk_size`, an oversized unit falls back to the character chunker, and each chunk
records its symbol name as the section label. Language is picked from the file
extension; non-code files keep the prose path. Logic in `maranode-rag/chunk.rs`
(`chunk_code`).

**Code vulnerability scan** — **[Done]**. A dependency-free heuristic scanner in
`maranode-rag/codescan.rs` flags common insecure patterns per line: hard-coded
credentials (incl. private-key blocks and AWS key ids), weak hashes (MD5/SHA-1),
SQL built by string concatenation or f-strings (parameterised queries are left
alone), dynamic code or shell execution (`eval`, `os.system`, `shell=True`),
disabled TLS verification (`verify=False`, `InsecureSkipVerify: true`, …), and
unsafe deserialization (`pickle.loads`, `yaml.load`). Each finding carries a rule
id, severity, line number and snippet. Exposed offline as `maranode scan <path>`
(`--min-severity`). It is a review hint, not a substitute for a full SAST tool;
broader code intelligence (syntax-aware search, code Q&A) stays on the roadmap.

**Local fine-tuning workflow** — **[Planned]**. A fine-tuning path that never sends
data out is not built.

---

## 19. Grounding proof for RAG

**Source binding in the receipt** — **[Done]**. When RAG is used, the receipt
carries a `sources` array. Each entry records `chunk_id`, `doc_id`, `source` (path
or URL), `doc_sha256` (hash of the full document at ingest), `chunk_hash` (hash of
the chunk at ingest) and the cosine `score` at retrieval. The whole receipt is
Ed25519-signed, so the source list can not be forged.

**Grounded flag** — **[Done]**. The receipt has `grounded: true` when at least one
source passed the `min_score` threshold, `grounded: false` when the answer came
from parametric memory only.

**Tamper detection on sources** — **[Done]**. `maranode audit verify-sources
<request_id>` opens the live RAG store, re-hashes each referenced chunk and reports
any mismatch. So if a source document was changed or re-ingested after the
inference, the change is detected. Endpoint side: the receipt's `sources` let a
third party re-hash their own copy by hand too.

**Honest limit (parametric leakage)** — grounding proves these chunks were in the
context window and the scores, signed. It does not and can not prove the answer was
derived only from those chunks, because the model still has parametric memory. This
is a fundamental LLM property and the docs state it plainly (`docs/grounding.md`).

---

## 20. Multi-tenant workspaces

**Workspaces** — **[Done]**. Isolated tenants inside one daemon. Each workspace has
its own API key, model allowlist, rate limit, system prompt, audit log segment and
encryption key (DEK). Useful for a hospital separating departments, a law firm
separating clients, a SaaS separating customers. Types in
`maranode-common/workspace.rs`, store in `maranode-store/workspace_db.rs`.

**Workspace CRUD** — **[Done]**. List, get, create, update and delete workspaces.
Handlers `list_workspaces`, `get_workspace`, `create_workspace`,
`update_workspace`, `del_workspace` in `maranode-api/routes/workspaces.rs`.

**Per-workspace quotas** — **[Done]**. `max_concurrent_requests`, `max_models`,
`max_memory_bytes` per workspace, enforced at runtime.

**Per-workspace audit segment** — **[Done]**. A workspace gets its own audit log
segment, exportable on its own with `GET /v1/audit/bundle/:workspace`.

**Per-workspace system prompt** — **[Done]**. Each workspace can set the system
prompt prepended to its conversations.

**Per-workspace crypto-shred** — **[Done]**. See section 10, this is the erasure
unit.

---

## 21. Users, authentication and SSO

**Local user accounts** — **[Done]**. `maranode users list | create | set-password
| disable | enable | delete`. Store in `maranode-store/user_db.rs` (`UserDb`). API:
`GET /v1/users`, `POST /v1/users`, `PUT /v1/users/:id/password`.

**API-key identity** — **[Done]**. Identity is asserted with
`Authorization: Bearer <key>`. An admin key (`auth.admin_key`) grants full access
including RAG ingest. With no admin key the daemon runs in open development mode
(loopback, single user).

**Login / logout / me** — **[Done]**. `POST /v1/auth/login`, `POST
/v1/auth/logout`, `GET /v1/auth/me`. Provider list at `GET /v1/auth/providers`.

**Session management** — **[Done]**. `GET /v1/sessions` lists sessions, `DELETE
/v1/sessions` revokes the others, `DELETE /v1/sessions/:token_prefix` revokes one.
`SessionRecord` in `user_db.rs`.

**OIDC login** — **[Done]**. `GET /v1/auth/oidc/login` and `GET
/v1/auth/oidc/callback`. PKCE verifier handling was added (the "OIDC PKCE verifier
updated" commit). Identity logic in `maranode-api/routes/identity.rs`.

**LDAP / Active Directory** — **[Partial]**. `POST /v1/auth/ldap/login` works after
a bug fix, but it still needs a compile-time gate and more hardening.

**SAML SSO** — **[Done, basic]**. `GET /v1/auth/saml/login` and `POST
/v1/auth/saml/callback`. IdP XML signature verification was added (the "saml
signature verification added" commit). Treat as basic until more IdPs are tested.

**Password reset** — **[Done, basic]**. `POST /v1/auth/password-reset/request` and
`POST /v1/auth/password-reset/confirm`, with SMTP configuration for the
notification mail (the "smtp password reset" commit). SMTP setup is still being
completed.

**Per-IP rate limiting** — **[Done]**. The API rate-limits per client IP, added in
the "api rate limit per ip address" commit, which protects the auth endpoints.

**Role-based access control** — **[Done]**. Named permissions (`chat`,
`rag_ingest`, `model_manage`, `audit_view`, `audit_export`, `audit_prune`,
`user_manage`, `workspace_manage`, and more) are the single source of truth in
`maranode-common/user.rs`; each role maps to a fixed set via `Role::permissions()`,
and the legacy `can_*` helpers delegate to `Role::has()`. Four roles ship: **admin**
(everything), **operator** (chat, RAG, model management, audit view), **auditor**
(chat plus audit view and compliance export — separation of duties, cannot prune or
manage), and **viewer** (chat only). Routes enforce a specific permission through
`authorize_permission()` / `UserCtx::require()`; the host admin key still passes as a
super-user and, with no admin key set, the daemon stays in open development mode.
Audit, model, workspace, DLP and incident endpoints are all wired to this — so an
operator or auditor session reaches what its role allows instead of needing the
master key, and the privileged incident/DLP actions (which previously accepted any
logged-in user) now require their named permission. `GET /v1/auth/me` returns the
caller's permission list.

---

## 22. API and interfaces

**HTTP API** — **[Done]**. Listens on `127.0.0.1:11984` by default (`bind` in
config). Built with axum, router assembled in `maranode-api/lib.rs`.

**Unix socket** — **[Done]**. The daemon also serves over a Unix domain socket
(`maranode-daemon/unix_serve.rs`), for local clients that do not want a TCP port.

**OpenAI-compatible surface** — **[Done]**. Chat, embeddings and models follow the
OpenAI shapes, so existing SDKs work by changing only the base URL. Mapping in
`maranode-api/openai.rs`.

**CLI** — **[Done]**. The `maranode` binary. Top-level commands: `model`, `audit`,
`verify`, `chat`, `rag`, `status`, `users`, `admin`, `serve`, `workspace`,
`baseline`, `registry`, `dlp`, `tpm`, `incident`, `hold`. Global flags `--host`
and `--data-dir`, env `MARANODE_HOST` / `MARANODE_DATA_DIR`. Full subcommand list
in section 29.

**Chat from the CLI** — **[Done]**. `maranode chat "<prompt>" --model <name:tag>`,
add `--rag` and `--collection` to ground in documents.

**Health and stats** — **[Done]**. `GET /health` and `GET /stats`. `maranode
status` prints daemon status and runtime stats, `maranode verify health` prints
the full health JSON.

**Web UI** — **[Partial]**. Served at `/ui` (`GET /ui`, `/ui/`, assets at
`/ui/assets/*`). Chat, model and audit views work, the rest of the UI is in active
development. Assets served from `maranode-api/routes/ui.rs`. Accessibility and the
mobile layout are done: skip link, nav/main landmarks, modal dialogs with focus
trap, Escape and focus restore, `aria-current` navigation, named icon buttons,
decorative icons hidden from screen readers, `aria-live` status and toast, plus an
off-canvas sidebar with a hamburger toggle and responsive breakpoints for narrow
screens. Most of this wiring is applied at runtime in `ui/assets/app.js`.

**Approval UI** — **[Done]**. The model approval workflow has its own web view at
`/v1/registry/ui`, see section 13.

**Prometheus metrics** — **[Done]**. `GET /metrics` returns the Prometheus text
format. It is off by default; set `[metrics] enabled = true` to turn it on, and it
stays behind the admin key unless `[metrics] require_auth = false`. Exposes request
and error counters, prompt/response token counters, summed handler duration, plus
gauges for uptime, inference queue depth, last audit sequence, workspace count, and
the air-gap, isolation and audit-freeze flags. Bind the daemon to localhost or keep
it behind the air-gap so the endpoint is not reachable from outside the host.
Encoder and route in `maranode-api/routes/metrics.rs`.

**Native TLS** — **[Done]**. Set `tls_cert` and `tls_key` in config, or pass
`--tls-cert` / `--tls-key` (env `MARANODE_TLS_CERT` / `MARANODE_TLS_KEY`), and the
daemon serves HTTPS on `bind` directly with rustls, so no reverse proxy is needed.
The PEM cert chain and private key (PKCS#8, PKCS#1 or SEC1) are read at start; the
process fails fast when a file is missing or the key does not match the cert. Both
options must be given together, and the Unix socket stays plain. Serving path in
`maranode-daemon/tls_serve.rs`.

---

## 23. Operations and deployment

**Single binary, no external services** — **[Done]**. `maranoded` and `maranode`
from one codebase. No Python, no database server, no sidecar. State is SQLite plus
flat files, so backup is a file copy and there is no extra service to patch.

**TOML config with env and flag overrides** — **[Done]**. Priority is defaults,
then config file, then env vars, then CLI flags (a flag always wins). Config search
order: `~/.config/maranode/config.toml`, then `/etc/maranode/config.toml`, or an
explicit `--config` / `MARANODE_CONFIG`. Full reference in section 26 and
`docs/config.toml.example`. Loader in `maranode-daemon/config.rs`.

**Hot config reload** — **[Done]**. Most settings apply without a restart:
`SIGHUP` to the daemon, or `maranode admin config-reload` (which calls the admin
endpoint). Event `config_reloaded`. Logic in `maranode-daemon/reload.rs`.

**Graceful shutdown** — **[Done]**. Clean shutdown that drains in-flight work,
`maranode-daemon/shutdown.rs`. Writes `daemon_stop`.

**systemd service** — **[Done]**. A `maranoded.service` unit plus a setup script in
`packaging/systemd/`.

**Admin operations** — **[Done]**. `maranode admin config-reload`. Admin endpoints
require `auth.admin_key` when one is set (`MARANODE_ADMIN_KEY`).

**Serve wrapper** — **[Done]**. `maranode serve [-- daemon args]` execs `maranoded`
with passthrough arguments.

**Multi-node / leader election** — **[Planned]**. No clustering yet.

**Shared model cache across nodes** — **[Planned]**.

**Zero-downtime upgrades** — **[Planned]**. Restart causes a brief gap today.

**Hardened minimal-OS appliance** — **[Planned]**. ISO/VM image, immutable root,
secure boot are roadmap items.

---

## 24. Packaging and distribution

**Docker images** — **[Done]**. CPU (`Dockerfile`), GPU (`Dockerfile.gpu`), ROCm
(`Dockerfile.rocm`), OpenVINO (`Dockerfile.openvino`), plus `docker-compose.yml`.

**Linux install script** — **[Done]**. `scripts/install.sh` for Ubuntu, Debian,
RHEL/Rocky/Alma, Alpine, Fedora, Arch. Hosting it at `get.maranode.com` is still a
TODO.

**Debian package** — **[Done]**. `packaging/debian/` with control, postinst, prerm
and the service unit. apt repository instructions in the README.

**RPM package** — **[Done]**. `packaging/rpm/maranode.spec` (the "rpm and arch
packagings added" commit). A dnf/yum repository is still planned.

**Arch package** — **[Done]**. `packaging/arch/PKGBUILD` and install script. A
pacman repository is still planned.

**Homebrew tap** — **[Done]**. `packaging/homebrew/maranode.rb`, for macOS
development.

**Reproducible build script** — **[Partial]**. Present in `scripts/`. Verifiable
binary-matches-source is the goal, not yet fully proven.

**Signed releases (Sigstore/cosign)** — **[Planned]**.

**Third-party security audit / SOC 2 / pen test** — **[Planned]**. These are
process items, not code, tracked in MOAT-ROADMAP.

---

## 25. Security posture and guarantees

**Fail-closed design** — **[Done]**. No isolation, no inference. No audit write, no
inference. Checksum mismatch, no load. Drifted baseline or uncertain isolation,
refuse and record. This is the core stance, not a setting.

**No telemetry, no phone-home** — **[Done]**. No update checker, no usage beacon.
The binary talks to nothing unless you tell it to. You can confirm with the egress
controls and your own `tcpdump`.

**Prompts hashed, not stored** — **[Done]**. See section 8. Content is opt-in only.

**Operator is not fully trusted** — the whole design assumes you may need to prove
something to an outside party who thinks you might be lying. That is why receipts,
the chain, legal hold and the separate compliance key exist.

**Threat model is written down** — **[Done]**. `docs/threat-model.md` and the
Trust Model section of `ARCHITECTURE.md` state what is guaranteed, what is depended
on, and what is explicitly out of scope (root attacker tampering rules and probe in
the same window, kernel/hardware exfiltration, parametric leakage). The handbook
keeps those honest limits next to each feature.

---

## 26. Configuration reference

From `docs/config.toml.example`. All settings have defaults, an empty config is
valid.

Core: `data_dir`, `bind` (env `MARANODE_BIND`), `log_level` (env `RUST_LOG`),
`device` (env `MARANODE_DEVICE`: `auto|cpu|gpu|npu|ryzenai`).

`[inference]`: `max_parallel` (default 4, each slot keeps its own KV cache),
`max_queue_depth` (default 32, 0 = unlimited; overflow returns HTTP 503). Both need
a restart.

`[assistant]`: `name`, `system_prompt` (inline), `system_prompt_file`. Resolution
order is file, then inline, then auto-generated from name, then nothing. The system
prompt is prepended to every conversation before any RAG context.

`[isolation]`: `mode` (`airgap|whitelist|disabled`), `api_port`, `allowed_sources`
(default loopback), and `[[isolation.whitelist]]` blocks (`host`, `port`,
`comment`) for whitelist mode. `--no-isolation` forces disabled.

`[auth]`: `admin_key` (env `MARANODE_ADMIN_KEY`, `--admin-key`). Unset means open
development mode.

`[rag]`: `enabled` (env `MARANODE_RAG`), `embedding_model`, `default_collection`,
`chunk_size` (1200), `chunk_overlap` (200), `top_k` (5), `min_score` (0.0),
`max_context_chars` (6000), `ingest_policy` (`anyone|admin_only|allowlist`),
`ingest_allowlist`.

---

## 27. Tests, demos and benchmarks

**Integration and e2e tests** — **[Done]**. In `tests/`: `classification_tests`,
`inference_test`, `store_test`, `store_security_test`, `api_router_test`,
`shred_test`, `repro_ci_test`, `audit_chain_test`, `tpm_tests`, `rag_test`,
`device_selection_test`, `drift_test`, and `e2e/api_test`. These cover the
sensitive paths: erasure, reproducibility, chain integrity, drift, TPM,
classification.

**Auth-flow tests** — **[Done]**. Mock-free unit tests for the parts that do not
need a live identity provider: `maranode-store/user_db.rs` exercises local
password hashing/verification, session create/resolve/expire/revoke and
disabled-account rejection, SSO identity lookup by `provider_sub`, and single-use
password-reset tokens against a temporary SQLite database; `routes/identity.rs`
tests the SAML assertion parser and XMLDSig helpers (PEM stripping, element
extraction, `SignedInfo` slicing, and signature rejection on incomplete
assertions). The live OIDC/LDAP/SAML round-trips still need a real IdP or an HTTP
mock harness, which stays a follow-up.

**Proof demo** — **[Done]**. `demos/proof-test/` is a runnable end-to-end demo of
proof-carrying inference: `run-inference.sh`, `sign-receipt.sh`,
`verify-receipt.sh`, `demo.sh`, with its own README. Good for showing the receipt
flow to someone.

**Benchmark tool** — **[Done]**. Crate `maranode-bench` (`maranode-bench/main.rs`)
for throughput/latency measurement.

---

## 28. What is not built yet

Kept honest so "do you have X?" gets a true answer.

**Tier 3 (tracked, not staffed)** — **[Planned]**:
- Zero-knowledge compliance proofs over the audit log (prove a statement about the
  log without revealing it).
- Federated cross-organization audit verification (consortium proofs with no raw
  data shared).

**Operations** — **[Planned]**: multi-node with leader election, shared model
cache, zero-downtime upgrades, minimal hardened OS image (immutable root, secure
boot, OVA/QCOW2).

**Runtime** — **[Planned]**: split one inference across CPU+GPU, exact
per-tokenizer budgeting.

**RAG** — **[Planned]**: code-aware chunking and code intelligence, local
fine-tuning workflow.

**Access** — **[Planned]**: role-based access control with fine-grained
permissions.

**Partial today** (works, has a named gap): Intel OpenVINO NPU and AMD Ryzen AI
builds (need cmake-flag build + testing), per-workspace network namespace
enforcement, manual-firewall-change detection, LDAP hardening + compile gate, full
web UI, reproducible-build verification.

**Ecosystem (speculative)** — **[Planned]**: sandboxed plugin marketplace, vertical
solutions (legal/defense/clinical), partner/hardware bundles, compliance-as-a-service.

---

## 29. Quick lookup: CLI commands

```
maranode model    pull | import | list | remove | quant (inspect|recommend|list)
maranode audit    verify | tail | export | bundle | prune | backup | restore
                  prove | replay | verify-sources | export-cert | forward
                  isolation-report
maranode verify   network | health | attest
maranode chat     "<prompt>" [--model] [--rag] [--collection]
maranode rag      add | list | search
maranode status
maranode users    list | create | set-password | disable | enable | delete
maranode admin    config-reload
maranode serve    [-- <daemon args>]
maranode workspace shred
maranode baseline create | sign | verify | list | fetch | check
maranode registry submit | list | tokens | approve | revoke
                  export-token | import-token | verify-token | ui | hooks-test
maranode dlp      sync (--provider purview|forcepoint|symantec)
maranode tpm      status | capture-pcrs | seal | unseal-test | verify-pcrs
                  export-recovery | import-recovery | rotate | rotation-log
                  tee-keygen | tee-probe
maranode incident declare | investigate | resolve | status | snapshot
                  bg-generate | bg-use
maranode hold     generate-key | place | sign-release | release | list
```

---

## 30. Quick lookup: HTTP endpoints

```
GET    /health
GET    /stats
GET    /ui  /ui/  /ui/assets/*path

POST   /v1/chat/completions
POST   /v1/embeddings
GET    /v1/models
GET    /v1/models/details
DELETE /v1/models/:model_id

GET    /v1/attestation/report
GET    /v1/attestation/public-key
GET    /v1/attestation/tee
POST   /v1/attestation/tee/verify
GET    /v1/attestation/tee/perf

GET    /v1/audit/entries
GET    /v1/audit/export
GET    /v1/audit/bundle
GET    /v1/audit/bundle/:workspace
POST   /v1/audit/prune
GET    /v1/audit/signing-key

POST   /v1/baseline/check

POST   /v1/registry/submit
GET    /v1/registry/pending
GET    /v1/registry/tokens
POST   /v1/registry/approve/:sha256
POST   /v1/registry/revoke/:sha256
POST   /v1/registry/hooks/test
GET    /v1/registry/ui

GET    /v1/classification/policy
PUT    /v1/classification/collections/:name   (DELETE to remove)
PUT    /v1/classification/workspaces/:slug
POST   /v1/dlp/sync

POST   /v1/incident/declare
POST   /v1/incident/investigate
POST   /v1/incident/resolve
GET    /v1/incident/status
POST   /v1/incident/snapshot
POST   /v1/incident/break-glass/generate
POST   /v1/incident/break-glass/use

POST   /v1/legal-hold/generate-key
POST   /v1/legal-hold/place
POST   /v1/legal-hold/sign-release
POST   /v1/legal-hold/release/:id
GET    /v1/legal-hold/list

POST   /v1/rag/documents
POST   /v1/rag/documents/upload
GET    /v1/rag/documents/:id/summary
POST   /v1/rag/documents/:id/summarize
POST   /v1/rag/extract
GET    /v1/rag/collections
DELETE /v1/rag/collections/:name
GET    /v1/rag/collections/:name/documents
POST   /v1/rag/search

POST   /v1/auth/login
POST   /v1/auth/logout
GET    /v1/auth/me
GET    /v1/auth/providers
GET    /v1/auth/oidc/login
GET    /v1/auth/oidc/callback
POST   /v1/auth/ldap/login
GET    /v1/auth/saml/login
POST   /v1/auth/saml/callback
POST   /v1/auth/password-reset/request
POST   /v1/auth/password-reset/confirm

GET    /v1/users
POST   /v1/users
PUT    /v1/users/:id/password
GET    /v1/sessions            (DELETE to revoke others)
DELETE /v1/sessions/:token_prefix

GET    /v1/workspaces          (POST to create)
PUT    /v1/workspaces/:slug    (GET to read, DELETE to remove)
```

---

*Cross-checked against the source tree, CLI definitions, HTTP routes, the audit
event enum and git history as of June 2026. Status flags reflect the code, not the
older snapshot in FEATURE-LIST.md. Pre-alpha, so it keeps moving.*

