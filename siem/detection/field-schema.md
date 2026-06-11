# Maranode audit field schema

This document covers all audit event types, their fields, and the SIEM-specific field names produced by each export format. Use it when writing detection rules or building a data connector.

## Export formats

| Format | Command | Output |
|--------|---------|--------|
| CEF    | `maranode audit export --format cef` | `.cef` file, one event per line |
| LEEF   | `maranode audit export --format leef` | `.leef` file, one event per line |
| Syslog | `maranode audit forward --target host:port` | RFC 5424 syslog over TCP or UDP, CEF body |
| Filebeat | JSONL tail via `filebeat.yml` | Elastic index via ingest pipeline |

---

## Common fields (all events)

| JSONL field | CEF extension | LEEF attribute | ECS field | Description |
|-------------|---------------|----------------|-----------|-------------|
| `ts` | `rt` (epoch ms) | `devTime` | `@timestamp` | Event timestamp (RFC 3339) |
| `seq` | `cn1` / `cn1Label=auditSeq` | `auditSeq` | `event.sequence` | Monotonically increasing chain sequence |
| `actor` | `src` | `usrName` | `user.name` | Actor that generated the event (`api`, `probe`, workspace slug) |
| `hmac` | — | — | `maranode.hmac` | HMAC-SHA256 chain link (hex) |
| `event` | `act` | `eventType` | `event.action` | Event type string (see table below) |

---

## Event types

### `daemon_start`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `version` | string | — | — | Daemon version string |
| `air_gap` | bool | — | — | Whether air-gap mode was active at start |

CEF sig_id: `MRN-001`  severity: 2

---

### `daemon_stop`

| Field | Type | Notes |
|-------|------|-------|
| `reason` | string | Shutdown reason |

CEF sig_id: `MRN-002`  severity: 2

---

### `isolation_applied`

| Field | Type | Notes |
|-------|------|-------|
| `mode` | enum | `air_gap`, `whitelist`, or `disabled` |

CEF sig_id: `MRN-010`  severity: 5

---

### `isolation_probe`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `isolated` | bool | `outcome=success/failure` | `isolated`, `outcome` | `false` means egress detected |
| `probe_results` | array | — | — | List of `{host, port, reachable}` objects |
| `iptables_hash` | string | `cs3` / `cs3Label=iptablesHash` | — | SHA-256 of `iptables-save` output (Linux only, else empty) |

CEF sig_id: `MRN-011`  severity: 5

Detection note: `isolated=false` is the signal of interest. A gap of >15 minutes without any `isolation_probe` event also warrants investigation when air-gap mode is expected to be active.

---

### `model_imported`

| Field | Type | CEF | Notes |
|-------|------|-----|-------|
| `model.name` | string | `cs1` / `cs1Label=modelName` | Model name component |
| `model.tag` | string | — | Model tag component |
| `sha256` | string | `fileHash` | SHA-256 of the model file |
| `size_bytes` | u64 | — | File size |
| `source` | enum | — | `remote` or `local` |

CEF sig_id: `MRN-020`  severity: 3

---

### `model_removed`

| Field | Type | CEF | Notes |
|-------|------|-----|-------|
| `model.name` | string | `cs1` | Model name |

CEF sig_id: `MRN-021`  severity: 3

---

### `inference_start`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `request_id` | string | `externalId` | `requestId` | UUID per inference call |
| `model.name` | string | `cs1` | — | |
| `prompt_sha256` | string | `cs2` / `cs2Label=promptSha256` | `promptSha256` | SHA-256 of full prompt text |
| `prompt` | string? | — | — | Only present when `log_prompts=true` |

CEF sig_id: `MRN-030`  severity: 2

---

### `inference_complete`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `request_id` | string | `externalId` | `requestId` | |
| `tokens_in` | u32 | — | `tokensIn` | |
| `tokens_out` | u32 | — | `tokensOut` | |
| `duration_ms` | u64 | — | — | Wall-clock time |
| `response` | string? | — | — | Only when `log_prompts=true` |

CEF sig_id: `MRN-031`  severity: 2

---

### `inference_failed`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `request_id` | string | `externalId` | `requestId` | |
| `reason` | string | — | — | Human-readable failure reason |

CEF sig_id: `MRN-032`  severity: 7

---

### `inference_receipt`

| Field | Type | Notes |
|-------|------|-------|
| `receipt.request_id` | string | Matches `inference_start` / `inference_complete` |
| `receipt.output_sha256` | string | SHA-256 of model output |
| `receipt.sources` | array | RAG source chunk references (may be empty) |
| `receipt.grounded` | bool | Whether RAG sources were used |
| `receipt.signature` | string | Ed25519 signature over receipt fields (base64) |

CEF sig_id: `MRN-033`  severity: 3

---

### `rag_document_ingested`

| Field | Type | Notes |
|-------|------|-------|
| `collection` | string | RAG collection name |
| `source` | string | File name or URL |
| `chunks` | usize | Number of chunks stored |

CEF sig_id: `MRN-040`  severity: 3

---

### `rag_retrieval`

| Field | Type | Notes |
|-------|------|-------|
| `collection` | string | |
| `query_sha256` | string | SHA-256 of the query text |
| `hits` | usize | Number of chunks retrieved |

CEF sig_id: `MRN-041`  severity: 2

---

### `workspace_shredded`

| Field | Type | CEF | LEEF | Notes |
|-------|------|-----|------|-------|
| `slug` | string | `fname` | `resourceId` | Workspace identifier |
| `actor` | string | `src` | `usrName` | Who initiated the shred |
| `statement` | string | — | — | Plain-text deletion statement for the certificate |

CEF sig_id: `MRN-050`  severity: 8

---

### `config_reloaded`

| Field | Type | Notes |
|-------|------|-------|
| `path` | string | Config file path |

CEF sig_id: `MRN-060`  severity: 3

---

### `audit_verified`

| Field | Type | Notes |
|-------|------|-------|
| `entries` | u64 | Number of entries checked |
| `ok` | bool | `false` means the HMAC chain is broken |

CEF sig_id: `MRN-070`  severity: 2 (ok=true), 9 (ok=false)

---

### `binary_attested`

| Field | Type | CEF | Notes |
|-------|------|-----|-------|
| `binary_sha256` | string | `fileHash` | SHA-256 of the running daemon binary |
| `binary_path` | string | — | Path of the binary |
| `tpm_available` | bool | — | Whether TPM PCR values were read |

CEF sig_id: `MRN-080`  severity: 4

---

## Severity reference (CEF scale 0–10)

| Severity | Meaning in Maranode context |
|----------|-----------------------------|
| 2 | Informational — normal operations |
| 3 | Low — expected administrative actions |
| 4 | Low-medium — attestation, worth indexing |
| 5 | Medium — isolation mode changes |
| 7 | High — inference failures |
| 8 | Critical — data destruction (shred) |
| 9 | Critical — audit chain integrity violation |
