# Verification Guide

This document explains how to independently verify Maranode's isolation guarantees using standard Linux tools - without trusting any Maranode-specific command.

The guiding principle: **every security claim must be verifiable with tools the operator already has and trusts**.

---

## 1. Verify network isolation

### Using `maranode verify`

```bash
maranode verify network
# -> ✓ Air-gap mode is ACTIVE - outbound traffic blocked.
```

### Independently with iptables

```bash
# Show all rules
sudo iptables -L -n -v

# What you should see in air-gap mode:
# Chain INPUT   (policy DROP)
# Chain OUTPUT  (policy DROP)
# Chain FORWARD (policy DROP)
# Plus: ACCEPT rules for loopback and the API port from allowed sources
```

### Independently with tcpdump

```bash
# Capture all non-loopback traffic for 30 seconds
sudo tcpdump -i any not lo -w /tmp/capture.pcap &
sleep 30
kill %1

# Analyse - should be empty (or only LAN traffic if you explicitly configured it)
tcpdump -r /tmp/capture.pcap | head
```

### Active probe (should fail)

```bash
# This should time out if air-gap is active
curl --connect-timeout 5 https://api.openai.com/v1/models
# -> curl: (28) Connection timed out after 5001 milliseconds
```

---

## 2. Verify audit log integrity

### Using `maranode audit verify`

```bash
maranode audit verify
# -> ✓ Audit log OK - 4,231 entries, HMAC chain intact.
```

### Verify the key file permissions

```bash
ls -la /var/lib/maranode/audit.key
# -> -rw------- 1 maranode maranode 32 May 21 2026 /var/lib/maranode/audit.key
```

### Inspect raw log entries

The log is plain JSON Lines - you can read it with any text tool:

```bash
tail -5 /var/lib/maranode/audit.jsonl | python3 -m json.tool
```

### Simulate tampering detection

```bash
# Make a backup
cp /var/lib/maranode/audit.jsonl /tmp/audit-original.jsonl

# Modify an entry
sed -i '5s/inference.complete/inference.complete_TAMPERED/' /var/lib/maranode/audit.jsonl

# Verify - should fail
maranode audit verify
# -> ✗ Audit log INTEGRITY VIOLATION detected!
#   At sequence 5: HMAC mismatch - entry has been tampered with

# Restore
cp /tmp/audit-original.jsonl /var/lib/maranode/audit.jsonl
```

---

## 3. Verify model integrity

```bash
# List models with their checksums
maranode model list

# Independently verify a specific blob
sha256sum /var/lib/maranode/blobs/sha256-<hash>
# The output should match the sha256 shown by `maranode model list`
```

---

## 4. Verify no unexpected processes or connections

```bash
# Show all open network connections by maranode processes
ss -tp | grep maranode

# Show all files opened by maranoded
lsof -p $(pgrep maranoded)

# Confirm no unexpected listening sockets
ss -tlnp | grep maranoded
```

---

## 5. Verify the binary

Every release artifact is signed with [cosign](https://docs.sigstore.dev/cosign/installation/) using keyless OIDC signing via GitHub Actions. No long-lived private key is stored.

```bash
ARCHIVE=maranode-v0.1.0-x86_64-unknown-linux-gnu.tar.gz

# Verify the cosign signature against the Sigstore transparency log
COSIGN_EXPERIMENTAL=1 cosign verify-blob \
  --certificate ${ARCHIVE}.crt \
  --signature   ${ARCHIVE}.sig \
  --certificate-identity-regexp "https://github.com/maranode/maranode/.github/workflows/" \
  --certificate-oidc-issuer "https://token.actions.githubusercontent.com" \
  ${ARCHIVE}
# -> Verified OK
```

### Reproduce the build locally

All release builds set `SOURCE_DATE_EPOCH` to the Git commit timestamp, making the output deterministic for the same source tree:

```bash
# Clone the exact release tag
git clone --branch v0.1.0 https://github.com/maranode/maranode
cd maranode

# Build with the reproducibility script
./scripts/build-release.sh

# Compare checksums against the official release
sha256sum target/release/maranoded target/release/maranode
# These should match the .sha256 files published in the GitHub Release.
```

---

---

## 6. Runtime integrity attestation

Maranode measures its own binary at startup and can produce a cryptographic attestation report combining the binary hash, TPM PCR values, and audit log chain status.

### Generate an attestation report (CLI)

```bash
# Print to stdout
maranode verify attest

# Save to file
maranode verify attest --output /tmp/attest.json
```

### Remote attestation endpoint

A third party can fetch a live report directly from the running daemon - no operator access required:

```bash
# Fetch without a nonce (replay risk)
curl http://maranode.example.com:11984/v1/attestation/report | jq .

# Fetch with a caller-supplied nonce for freshness
NONCE=$(openssl rand -hex 16)
curl "http://maranode.example.com:11984/v1/attestation/report?nonce=${NONCE}" > report.json

# Confirm the nonce is echoed back
jq -r '.nonce' report.json   # should equal $NONCE
```

The response includes all the same fields as the CLI report, plus:

| Field | Purpose |
|-------|---------|
| `nonce` | Echo of the caller's nonce (empty if not supplied) |
| `hmac` | HMAC-SHA256 of `report_json + nonce`, keyed with the instance's audit key |
| `signed` | `true` if the audit key was available for signing |

The `hmac` field binds the attestation to the same cryptographic identity as the audit log chain. A verifier who has obtained the audit key out-of-band (e.g. from an air-gapped operator or a prior trust-establishment step) can verify it independently:

```python
import hmac, hashlib, json

key = bytes.fromhex(open("audit.key").read().strip())
with open("report.json") as f:
    resp = json.load(f)

# reconstruct the signed payload
inner = {k: v for k, v in resp.items() if k not in ("nonce", "hmac", "signed")}
payload = json.dumps(inner, separators=(',', ':')) + resp["nonce"]
expected = hmac.new(key, payload.encode(), hashlib.sha256).hexdigest()

assert expected == resp["hmac"], "HMAC mismatch - report may be forged or stale"
print("Attestation verified")
```

The report includes:

- **Binary measurement** - SHA-256 of the running `maranoded` executable
- **TPM PCR values** - SHA-256 PCR registers 0–23 read directly from `/dev/tpmrm0` via TPM2_PCR_Read (no `libtss2` dependency; falls back gracefully to "unavailable" if no TPM is present)
- **Audit log status** - entry count and HMAC chain verification result
- **Report hash** - SHA-256 of the whole report body, so a third party can sign or re-verify it

Example output (abbreviated):

```json
{
  "version": 1,
  "generated_at": "2026-06-04T12:00:00Z",
  "binary": {
    "path": "/usr/bin/maranoded",
    "sha256": "a3f2e9c1...",
    "size_bytes": 18432000
  },
  "tpm": {
    "status": "available",
    "pcrs": {
      "0": "3d458cfe...",
      "1": "b2a5c9f0...",
      "14": "0000000000000000000000000000000000000000000000000000000000000000"
    }
  },
  "audit_log": {
    "path": "/var/lib/maranode/audit.jsonl",
    "sha256": "7c4e1b2d...",
    "entries": 4231,
    "chain_ok": true
  },
  "report_sha256": "f1e2d3c4..."
}
```

### Verify independently

```bash
# Confirm the binary hash matches what maranoded reports
sha256sum $(which maranoded)
# Should match report.binary.sha256

# Confirm PCR 14 (systemd EFI stub) is all zeros on an unmodified boot
jq '.tpm.pcrs["14"]' /tmp/attest.json
# "0000000000000000000000000000000000000000000000000000000000000000"

# Verify report hash
jq 'del(.report_sha256)' /tmp/attest.json | sha256sum
# Should match report.report_sha256
```

### Audit trail

Every daemon startup logs a `binary_attested` event into the audit log:

```jsonl
{"event":"binary_attested","binary_sha256":"a3f2e9c1...","binary_path":"/usr/bin/maranoded","tpm_available":true}
```

This means every restart is on record - if the binary hash changes between restarts, the audit log reflects it and the HMAC chain preserves the evidence.

### TPM availability

| Platform | Device | Result |
|----------|--------|--------|
| Linux + TPM 2.0 | `/dev/tpmrm0` or `/dev/tpm0` | PCRs read via direct TPM2_PCR_Read command |
| Linux, no TPM | - | `"status": "unavailable"` |
| macOS / Windows | - | `"status": "unavailable"` |

Root or `tss2` group membership is required to open the TPM device. On systemd systems, add the service user to the `tss` group:

```bash
usermod -aG tss maranode
```

---

## Summary checklist for auditors

| Item | Command | Expected result |
|------|---------|-----------------|
| Air-gap active | `iptables -L -n` | `policy DROP` on INPUT and OUTPUT |
| No external traffic | `tcpdump -i any not lo` | No packets |
| Audit chain intact | `maranode audit verify` | `HMAC chain intact` |
| Binary hash matches | `sha256sum $(which maranoded)` | Matches `maranode verify attest` report |
| TPM PCRs recorded | `maranode verify attest` | PCR values present (if TPM available) |
| Model checksums match | `sha256sum <blob>` | Matches stored sha256 |
| Binary signature valid | `cosign verify-blob ...` | `Verified OK` |
| API bound locally | `ss -tlnp` | `127.0.0.1:11984` only |
| Key permissions | `ls -la /var/lib/maranode/audit.key` | `0600` |
