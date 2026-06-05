# Compliance Tooling

Maranode's audit log is the foundation for compliance reporting. Every daemon action - inference requests, model imports, RAG operations, config changes, daemon start/stop - is written to an append-only, HMAC-chained log. This page covers how to export, package, and manage that data for regulatory requirements.

## Supported frameworks

| Format | Standard | Primary use |
|---|---|---|
| `gdpr` | GDPR Article 30 | Records of processing activities |
| `hipaa` | HIPAA § 164.312(b) | Access and disclosure audit controls |
| `soc2` | SOC 2 Trust Services Criteria | Security event log |
| `iso27001` | ISO/IEC 27001 Annex A | Information security event log |

Each export is a CSV file with framework-appropriate column headings, filtered and formatted from the raw audit log.

## Export via web UI

Open **Audit Log** in the sidebar and scroll to the **Compliance Export** card.

1. Choose a framework from the **Framework** dropdown.
2. Optionally filter by workspace slug, and/or provide a date range.
3. Click **Download CSV** to save the export to your computer.

To download an evidence bundle (see below), click **Download Evidence Bundle (.zip)**.

## Export via API

Requires an admin key (`Authorization: Bearer <key>`).

```bash
# GDPR export - all entries
curl -H "Authorization: Bearer $ADMIN_KEY" \
  "http://localhost:11984/v1/audit/export?format=gdpr" \
  -o audit_gdpr.csv

# HIPAA export - single workspace, date range
curl -H "Authorization: Bearer $ADMIN_KEY" \
  "http://localhost:11984/v1/audit/export?format=hipaa&workspace=clinic-a&from=2024-01-01T00:00:00Z&to=2024-12-31T23:59:59Z" \
  -o audit_hipaa.csv
```

**Query parameters**

| Parameter | Type | Description |
|---|---|---|
| `format` | string | `gdpr`, `hipaa`, `soc2`, or `iso27001` |
| `workspace` | string | Filter to entries where `actor` matches this workspace slug |
| `from` | RFC 3339 | Include entries at or after this timestamp |
| `to` | RFC 3339 | Include entries at or before this timestamp |

## Export via CLI

```bash
# GDPR export - all workspaces
maranode audit export --format gdpr --output audit_gdpr.csv

# SOC 2 - specific workspace, custom output path
maranode audit export --format soc2 --workspace clinic-a --output soc2_clinic_a.csv

# ISO 27001 - date range
maranode audit export --format iso27001 \
  --from 2024-01-01T00:00:00Z \
  --to   2024-06-30T23:59:59Z \
  --output iso27001_h1.csv
```

## Evidence bundles

An evidence bundle is a ZIP archive containing:

- `audit.jsonl` - the complete raw audit log
- `integrity.json` - HMAC chain verification result (entries checked, pass/fail, first violation if any)
- `manifest.json` - SHA-256 checksums of both files and bundle creation timestamp

Bundles are suitable for submission to auditors as HMAC-chained packages. The HMAC key used for verification is stored server-side at `<data-dir>/audit.key`.

```bash
# API
curl -H "Authorization: Bearer $ADMIN_KEY" \
  "http://localhost:11984/v1/audit/bundle" \
  -o audit_bundle.zip

# CLI
maranode audit bundle --output audit_bundle.zip
```

## Retention / pruning

Entries older than a threshold can be pruned to limit log growth. The HMAC chain is rewritten to remain valid after pruning.

**Dry run first:**

```bash
maranode audit prune --retain-days 90
# -> Dry run: 142 entries older than 90 days would be pruned. Re-run with --confirm to apply.
```

**Apply:**

```bash
maranode audit prune --retain-days 90 --confirm
# -> Pruned 142 entries older than 90 days.
```

**Via API:**

```bash
curl -X POST -H "Authorization: Bearer $ADMIN_KEY" \
  -H "Content-Type: application/json" \
  -d '{"retain_days": 90}' \
  "http://localhost:11984/v1/audit/prune"
# -> {"pruned": 142}
```

**Via web UI:**

Open **Audit Log -> Retention**, enter the number of days, and click **Prune Old Entries**. A confirmation dialog appears before any deletion occurs.

## Per-workspace exports

Every workspace has its own audit log stored at:

```
<data-dir>/workspaces/<slug>/audit.jsonl
```

The API and CLI `--workspace` filter works against the actor field in the global log (requests logged with the workspace slug as actor). For a complete per-workspace evidence bundle, use the audit files in the workspace subdirectory directly.

## Column reference

### GDPR Article 30

| Column | Description |
|---|---|
| timestamp | RFC 3339 event time |
| seq | Monotonic sequence number |
| actor | Workspace slug or `daemon` |
| activity | Event type (e.g. `inference_complete`) |
| detail | Key-value pairs of event fields |

### HIPAA Access Log

| Column | Description |
|---|---|
| timestamp | RFC 3339 event time |
| seq | Sequence number |
| actor | Workspace slug or `daemon` |
| event_type | Event type |
| access_detail | Relevant access detail (prompt hash, token counts, query hash) |
| phi_indicator | `yes` for events that may involve PHI (inference, RAG), `no` otherwise |

### SOC 2 Security Events

| Column | Description |
|---|---|
| timestamp | RFC 3339 event time |
| seq | Sequence number |
| actor | Workspace slug or `daemon` |
| category | `availability`, `change_management`, `logical_access`, or `monitoring` |
| event_type | Event type |
| detail | Key-value pairs of event fields |

### ISO 27001 Event Log

| Column | Description |
|---|---|
| timestamp | RFC 3339 event time |
| seq | Sequence number |
| actor | Workspace slug or `daemon` |
| control | ISO 27001 Annex A control reference (e.g. `A.9.4`, `A.12.4`) |
| event_type | Event type |
| outcome | Operational outcome (`access`, `change`, `failure`, `monitoring`, etc.) |
| detail | Key-value pairs of event fields |
