# Workspaces

Workspaces are isolated environments on a single Maranode instance. Each workspace has its own API key, model allowlist, rate limit, resource quotas, system prompt, and audit log. The underlying inference engine and model blobs are shared across all workspaces - workspaces separate *access* and *context*, not hardware resources.

## What a workspace does

| Capability | Per-workspace |
|---|---|
| API key authentication | ✓ Each workspace has its own key |
| Model allowlist | ✓ Restrict which models a workspace can use |
| Rate limiting | ✓ Cap requests per minute (`rate_limit_rpm`) |
| Concurrent request quota | ✓ Cap simultaneous in-flight requests (`max_concurrent_requests`) |
| Model count quota | ✓ Cap distinct models in use at once (`max_models`) |
| Memory quota | ✓ Cap total model file size loaded simultaneously (`max_memory_bytes`) |
| System prompt | ✓ Override the global system prompt |
| Audit log | ✓ Separate audit trail at `workspaces/<slug>/audit.jsonl` |
| Inference engine | Shared (GPU/CPU is a shared resource) |
| Model blobs | Shared (models are content-addressed, deduplicated) |

## Use cases

**Clinic** - Create one workspace per department (radiology, admin, oncology). Each gets a tailored system prompt and an allowlist restricting it to the approved model. Rate limits prevent one department from saturating the queue. Each department's conversations are logged separately, making HIPAA audit evidence collection clean.

**Bank** - Operations, compliance, and customer service each get a workspace. Compliance sets a system prompt instructing the assistant to cite sources and add disclaimers. Operations allows unrestricted model access. The compliance workspace API key is distributed only to approved tooling - other teams cannot use it even if they know the instance URL.

**Individual developer** - Use the default workspace for everything. No configuration needed; workspaces are opt-in.

## Default workspace

Every Maranode installation has a `default` workspace created automatically on first run. It has no API key, no model restrictions, and no rate limit. Requests without an `X-Maranode-Workspace` header are routed to it. This means existing integrations continue working with no changes.

## Authentication

Each request to the API identifies its workspace via two headers:

```
X-Maranode-Workspace: clinic-radiology
Authorization: Bearer <workspace-api-key>
```

If a workspace has no API key set, the `Authorization` header is optional. If it does have a key, requests without the correct key receive `401 Unauthorized`.

The admin key (set via `MARANODE_ADMIN_KEY` or `auth.admin_key` in config) bypasses workspace authentication and can access any workspace and the management endpoints.

## API

All workspace management endpoints require the admin key.

### List workspaces

```bash
curl http://localhost:11984/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY"
```

### Create a workspace

```bash
curl http://localhost:11984/v1/workspaces \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{
    "slug": "clinic-radiology",
    "name": "Radiology",
    "model_allowlist": ["llama3.2:3b"],
    "rate_limit_rpm": 30,
    "max_concurrent_requests": 5,
    "max_models": 2,
    "max_memory_bytes": 8589934592,
    "system_prompt": "You are a medical documentation assistant. Always recommend consulting a physician."
  }'
```

The response includes the workspace's API key. It is only returned once - store it immediately.

```json
{
  "workspace": {
    "slug": "clinic-radiology",
    "name": "Radiology",
    "has_key": true,
    "model_allowlist": ["llama3.2:3b"],
    "rate_limit_rpm": 30,
    "max_concurrent_requests": 5,
    "max_models": 2,
    "max_memory_bytes": 8589934592,
    "has_system_prompt": true,
    "net_namespace": false,
    "ns_active": false,
    "created_at": "2026-05-31T10:00:00Z"
  },
  "api_key": "a3f2e9c1..."
}
```

You can also supply your own key:

```bash
curl ... -d '{"slug": "ops", "name": "Operations", "api_key": "my-secret-key"}'
```

### Use a workspace

```bash
curl http://localhost:11984/v1/chat/completions \
  -H "X-Maranode-Workspace: clinic-radiology" \
  -H "Authorization: Bearer a3f2e9c1..." \
  -H "Content-Type: application/json" \
  -d '{"model": "llama3.2:3b", "messages": [{"role": "user", "content": "Summarise this scan report."}]}'
```

### Update a workspace

```bash
curl -X PUT http://localhost:11984/v1/workspaces/clinic-radiology \
  -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"rate_limit_rpm": 60, "max_concurrent_requests": 10}'
```

To remove a field, use the corresponding `clear_*` flag:

| Field | Clear flag |
|---|---|
| `rate_limit_rpm` | `"clear_rate_limit": true` |
| `max_concurrent_requests` | `"clear_max_concurrent_requests": true` |
| `max_models` | `"clear_max_models": true` |
| `max_memory_bytes` | `"clear_max_memory_bytes": true` |
| `system_prompt` | `"clear_system_prompt": true` |
| `api_key_hash` | `"clear_key": true` |

### Delete a workspace

```bash
curl -X DELETE http://localhost:11984/v1/workspaces/clinic-radiology \
  -H "Authorization: Bearer $ADMIN_KEY"
```

The `default` workspace cannot be deleted.

## Audit logs

Each workspace writes its inference events to a separate append-only HMAC-chained log:

```
/var/lib/maranode/workspaces/clinic-radiology/audit.jsonl
```

Verify integrity the same way as the global log:

```bash
maranode audit verify --workspace clinic-radiology
```

Or inspect it directly - the format is identical to the global audit log.

## Rate limiting

Rate limits are per-workspace, counted in a rolling 60-second window. When the limit is exceeded, the request receives `503 Service Unavailable`:

```json
{"error": {"message": "workspace 'clinic-radiology' rate limit of 30 rpm exceeded", "code": 503}}
```

## Resource quotas

Three quota fields bound how much of the shared inference engine a workspace can consume simultaneously. All three are optional - omit them for no limit.

### `max_concurrent_requests`

Maximum number of in-flight inference requests at the same time. A request that arrives when the workspace is already at capacity receives `503`:

```json
{"error": {"message": "workspace 'clinic-radiology' concurrent request limit of 5 exceeded", "code": 503}}
```

Use this to prevent one workspace from holding all queue slots and starving others.

### `max_models`

Maximum number of distinct models the workspace can have active at once. A model counts as active from the moment a request for it starts until the response (or stream) completes. If a second model would exceed the limit, the request receives `503`:

```json
{"error": {"message": "workspace 'clinic-radiology' simultaneous model limit of 2 exceeded", "code": 503}}
```

Use this alongside `model_allowlist` when you want to allow several models but prevent all of them from loading concurrently.

### `max_memory_bytes`

Maximum total model file size (GGUF bytes on disk) the workspace can have in use at once. This is a proxy for RAM/VRAM consumption - it does not account for context memory or KV cache. If loading an additional model would push the workspace over budget, the request receives `503`:

```json
{"error": {"message": "workspace 'clinic-radiology' memory quota of 8589934592 bytes would be exceeded", "code": 503}}
```

The same model used by multiple concurrent requests counts once toward the memory total.

**Example values:**

| Quota | Bytes |
|---|---|
| 4 GB | `4294967296` |
| 8 GB | `8589934592` |
| 16 GB | `17179869184` |

## Model allowlist

If `model_allowlist` is non-empty, the workspace can only use the listed models. Requests for other models receive `403 Forbidden`:

```json
{"error": {"message": "model 'gpt-4:latest' is not in the allowlist for workspace 'clinic-radiology'", "code": 403}}
```

## Web UI

The Workspaces page (sidebar -> Workspaces) lets you:

- View all workspaces with their configuration
- Create a new workspace with a form
- Switch the browser's active workspace from the topbar badge or the page

The topbar always shows which workspace the browser is currently sending requests to. Click it to switch.

Admin key and workspace keys are stored in browser `localStorage` - they stay between sessions but never leave the browser.

## Storage

Workspace configuration is stored in `/var/lib/maranode/workspaces.db` (SQLite). The file is created automatically on first run and is separate from the model manifest database.

API keys are stored as SHA-256 hashes. The plaintext key is never persisted.
