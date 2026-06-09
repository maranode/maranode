# Usage

## Starting the daemon

```bash
# Default (localhost, no auth, no RAG)
./target/release/maranoded
# equivalent:
./target/release/maranode serve

# With RAG
./target/release/maranoded --rag
# equivalent:
./target/release/maranode serve --rag

# Production (set an admin key before exposing to a network)
MARANODE_ADMIN_KEY="$(openssl rand -hex 32)" ./target/release/maranoded --rag
```

> Without `--admin-key` the daemon logs a warning and runs in open dev mode - all endpoints are unauthenticated. Fine for local use, not for networked deployments.

### Hot config reload

After editing `config.toml`, reload runtime settings without restarting:

```bash
# HTTP API (admin key required when auth.admin_key is set)
curl -X POST http://127.0.0.1:11984/v1/admin/config/reload \
  -H "Authorization: Bearer $MARANODE_ADMIN_KEY"

# CLI
maranode admin config-reload

# Linux: signal the daemon (same reload path)
kill -HUP $(pgrep maranoded)
```

**Applied without restart:** `auth`, `assistant` system prompt, RAG tuning, `isolation`, `inference.max_queue_depth`, `log_level`.

**Requires restart:** `bind`, `data_dir`, `device`, `rag.enabled`, `rag.embedding_model`, `unix_socket` (and any value overridden on the `maranoded` command line at startup).

## Model management

Models are separated into two types: **LLM** (chat/generation) and **embedding** (RAG only).

```bash
# Pull a model from Hugging Face (streaming download with progress bar)
maranode model pull bartowski/Llama-3.2-3B-Instruct-GGUF/Llama-3.2-3B-Instruct-Q4_K_M.gguf \
  --name llama3.2 --tag 3b --quant Q4_K_M

# Pull from a full URL
maranode model pull https://example.com/model.gguf --name mymodel --tag latest

# Import from a local file (air-gapped deployments)
maranode model import /path/to/llama3.2.gguf --name llama3.2 --tag 3b

# Import an embedding model
maranode model import /path/to/bge-m3.gguf --name bge-m3 --tag latest --type embedding

# List all models (shows type column)
maranode model list
```

`model pull` is blocked when the daemon is running in air-gap mode. Use `model import` for air-gapped deployments - download on an internet-connected machine, transfer via removable media, then import.

`GET /v1/models` (OpenAI-compatible) returns LLM models only. The web UI models page shows both, grouped by type. The chat model selector only shows LLM models - embedding models cannot be used for chat.

## Chat

```bash
# CLI
maranode chat "Summarize the financial tables of first quarter"

# HTTP (OpenAI-compatible)
curl http://localhost:11984/v1/chat/completions \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2:3b","messages":[{"role":"user","content":"Hello"}]}'
```

Python works with the standard OpenAI SDK - just set `base_url="http://localhost:11984/v1"`.

## RAG (Retrieval-Augmented Generation)

RAG lets the model answer from your own documents instead of guessing. It runs entirely locally.

**Setup:**

```bash
# 1. Start with RAG enabled
./target/release/maranoded --rag

# 2. Import an embedding model (must use --type embedding)
maranode model import /path/to/bge-m3.gguf --name bge-m3 --tag latest --type embedding

# 3. Add documents to the knowledge base
maranode rag add ./report.txt
maranode rag add ./contract.pdf --collection contracts
```

**Ask grounded questions:**

```bash
maranode chat "What was the patient's blood pressure?" --rag
```

The model answers from retrieved context only and cites sources like `[1]`. If the answer isn't in the documents it says so rather than guessing.

**Chat file attachments** work without RAG - drag a file into the web UI or attach via the API. The text is extracted and injected into that message only; nothing is stored permanently.

## Authentication

Set `--admin-key` (or `MARANODE_ADMIN_KEY`) to protect ingest endpoints:

```bash
# Ingest requires Bearer token
curl http://localhost:11984/v1/rag/documents/upload \
  -H "Authorization: Bearer $MARANODE_ADMIN_KEY" \
  -F "file=@report.txt"
```

### RAG ingest policy

Controls who can write to the persistent store. Set in `config.toml`:

```toml
[rag]
# "anyone" (default) | "admin_only" | "allowlist"
ingest_policy = "admin_only"
```

| Policy | Who can ingest |
|--------|----------------|
| `anyone` | No key required - for single-user / loopback use |
| `admin_only` | Admin key only - recommended for multi-user |
| `allowlist` | Admin key + listed service-account keys |

## User management

The `maranode users` subcommand operates directly on the local database - no daemon required.

```bash
maranode users list

maranode users create jane --role operator
maranode users create jane --role operator --email jane@co.com --password s3cr3t

maranode users set-password jane   # prompted interactively if --password omitted

maranode users disable jane        # blocks login, preserves audit trail
maranode users enable jane
maranode users delete jane
```

Roles: `admin`, `operator`, `viewer`. `set-password` refuses SSO accounts (OIDC/LDAP/SAML).

If the daemon is not running or you need to bootstrap before first start, use `--data-dir` to point at the right database:

```bash
maranode --data-dir /opt/maranode/data users create admin --role admin
```

See [docs/users.md](users.md) for the full reference including HTTP API, OIDC, LDAP, and SAML.

## Request queue

Maranode runs up to `inference.max_parallel` requests at the same time. Additional requests wait in a bounded queue. If the queue fills up, new requests are rejected immediately with HTTP 503. The current queue depth is visible in the web UI stats bar and at `GET /stats`.

Configure in `config.toml`:

```toml
[inference]
max_parallel    = 4    # requests running simultaneously (default 4)
max_queue_depth = 32   # reject with 503 if more than this many are waiting
```

**`max_parallel`** — default is 4. Each parallel slot holds its own KV cache in RAM (roughly 1-2 GB per slot for a 7B Q4 model on CPU). Lower this on memory-constrained machines; raise it for GPU deployments or high-concurrency servers. Requires a daemon restart to change.

**`max_queue_depth`** — default is 32. Set higher for many concurrent users, lower for single-user workstations. Set to `0` for unlimited (not recommended).

## Configuration file

Place at `/etc/maranode/config.toml` or pass with `--config`:

```toml
bind      = "127.0.0.1:11984"
log_level = "info"
device    = "auto"   # auto | cpu | gpu | npu

[auth]
admin_key = "change-me"

[rag]
enabled            = false
embedding_model    = "bge-m3:latest"   # recommended: multilingual, 100+ languages
default_collection = "default"
ingest_policy      = "admin_only"
```

See `docs/config.toml.example` for all options with comments.

## API reference

| Method | Path | Auth | Description |
|--------|------|------|-------------|
| `GET`  | `/health` | - | Health check |
| `GET`  | `/ui` | - | Web UI |
| `GET`  | `/v1/models` | - | List LLM models only (OpenAI-compatible) |
| `GET`  | `/v1/models/details` | - | List all models with type, size, SHA-256 |
| `POST` | `/v1/chat/completions` | - | Chat (OpenAI-compatible) |
| `POST` | `/v1/embeddings` | - | Embeddings |
| `POST` | `/v1/rag/extract` | - | Extract text from file (not stored) |
| `POST` | `/v1/rag/documents/upload` | ingest policy | Ingest file into RAG store |
| `GET`  | `/v1/rag/collections` | - | List collections |
| `POST` | `/v1/rag/search` | - | Search RAG store |
| `GET`  | `/v1/audit/entries` | - | Recent audit log entries |
| `GET`  | `/stats` | - | Runtime stats: requests, tokens, latency, queue depth |

## Workspaces

Workspaces are isolated environments within a single daemon - each with its own API key, model allowlist, rate limit, system prompt, and audit log. Useful for multi-tenant deployments (separate departments, clients, or applications sharing one server).

```bash
# Create a workspace (auto-generates an API key)
curl -X POST http://localhost:11984/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"slug":"clinic-a","name":"Clinic A"}'

# Use a workspace in API calls
curl http://localhost:11984/v1/chat/completions \
  -H "X-Maranode-Workspace: clinic-a" \
  -H "Authorization: Bearer <workspace-key>" \
  -H "Content-Type: application/json" \
  -d '{"model":"llama3.2:3b","messages":[{"role":"user","content":"Hello"}]}'
```

See [docs/workspaces.md](workspaces.md) for the full reference.

## Status and health

```bash
# Show daemon version, uptime, air-gap state, model count, and runtime stats
maranode status

# Verify network isolation is active (active probe - not just a flag check)
maranode verify network

# Full health JSON from the daemon
maranode verify health

# Generate a runtime integrity attestation report
maranode verify attest
maranode verify attest --output /tmp/attest.json   # save to file
```

`verify network` checks three things: the daemon's air-gap flag, the `iptables-save` OUTPUT policy (Linux only), and three live TCP probes to external IPs. If any probe succeeds, it exits with code 1.

`verify attest` produces a JSON report containing the SHA-256 of the running binary, TPM 2.0 PCR values (read directly from `/dev/tpmrm0` with no C library dependency - falls back gracefully if no TPM is present), audit log entry count, and HMAC chain status. The daemon also logs a `binary_attested` event on every startup so the binary hash is recorded in the immutable audit trail. See [docs/verification.md](verification.md) for the full report format and independent verification steps.

## Audit log

```bash
maranode audit verify      # check HMAC chain integrity
maranode audit tail -n 50  # show recent entries
```

## Compliance exports

Export the audit log as a framework-specific CSV, or download a signed ZIP evidence bundle for auditors.

```bash
# Export as GDPR Article 30 CSV
maranode audit export --format gdpr --output audit_gdpr.csv

# Export as HIPAA access log, filtered to one workspace
maranode audit export --format hipaa --workspace clinic-a --output hipaa_clinic_a.csv

# Download a ZIP evidence bundle (raw log + integrity report + SHA-256 manifest)
maranode audit bundle --output audit_bundle.zip

# Prune entries older than 90 days (dry run first, then --confirm)
maranode audit prune --retain-days 90
maranode audit prune --retain-days 90 --confirm
```

Supported frameworks: `gdpr`, `hipaa`, `soc2`, `iso27001`. The same exports are available via the web UI (Audit Log -> Compliance Export) and the HTTP API. See [docs/compliance.md](compliance.md) for details.
