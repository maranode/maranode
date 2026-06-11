# Inference Receipt — Format and Verification Guide

Every inference call in Maranode produces a signed receipt. The receipt proves what
model ran, what input it received, and what output it produced. A third party can
verify the receipt offline without access to the Maranode node.

---

## How to get a receipt

### From the API

Add `"with_receipt": true` to the chat completion request body:

```json
{
  "model": "llama3",
  "messages": [{"role": "user", "content": "Hello"}],
  "with_receipt": true
}
```

The response will include a `receipt` object at the top level. Save it to a file.

### From the audit log

If you did not request `with_receipt` at call time, the receipt is still written to
the audit log. Extract it with:

```
maranode audit prove <request_id> > receipt.json
```

The `request_id` is the `X-Request-Id` header returned by the API.

---

## Receipt format

A receipt is a JSON object. All fields are always present unless marked optional.

| Field | Type | Description |
|---|---|---|
| `version` | integer | Receipt format version. Currently `1`. |
| `receipt_id` | UUID string | Unique id for this receipt. |
| `request_id` | string | The HTTP request id (`X-Request-Id`). |
| `timestamp` | RFC 3339 string | When the inference completed (UTC). |
| `model_id` | string | Model name as registered in Maranode. |
| `model_sha256` | hex string | SHA-256 of the GGUF model file. |
| `model_quant` | string (optional) | Quantization label if known. |
| `input_sha256` | hex string | SHA-256 over the ordered input messages. |
| `output_sha256` | hex string | SHA-256 of the output text. |
| `decode_params` | object | See below. |
| `tokens_in` | integer | Prompt token count. |
| `tokens_out` | integer | Generated token count. |
| `signing_key_id` | hex string | 32-byte Ed25519 public key (64 hex chars). |
| `tpm_pcr` | string (optional) | TPM PCR composite quote, if available. |
| `signature` | hex string (optional) | Ed25519 signature over canonical bytes. |

### `decode_params` fields

| Field | Type |
|---|---|
| `temperature` | float or null |
| `top_k` | integer or null |
| `max_tokens` | integer or null |
| `seed` | integer or null |
| `deterministic` | boolean |

---

## What is signed

The signature covers all fields except `signature` itself. To get the signed bytes,
serialize the receipt to JSON with `signature` field removed (or set to null and
omitted via `skip_serializing_if`). The JSON must be compact (no extra whitespace)
and use the same field order as the Rust struct. This is what Maranode calls
"canonical bytes".

Algorithm: Ed25519.
Key: the `signing_key_id` field is the verifying key itself (not a key id lookup).

---

## How to verify — using the bundled tool

The `maranode-verify` binary is a standalone verifier. It has no dependency on the
running Maranode daemon or database.

Basic signature check:

```
maranode-verify receipt.json
```

With input/output file re-check:

```
maranode-verify receipt.json --input prompt.json --output response.txt
```

Exit code `0` means VERIFIED, exit code `1` means FAILED.

---

## How to verify — manually

If you want to verify without installing any Maranode binary, the steps are:

**1. Re-compute canonical bytes**

Take the receipt JSON, remove the `signature` field, and re-serialize compact JSON
in the same field order. In Python:

```python
import json, copy

with open("receipt.json") as f:
    r = json.load(f)

sig_hex = r.pop("signature")  # remove and save
canonical = json.dumps(r, separators=(",", ":")).encode()
```

Note: the field order in the JSON file as produced by Maranode is already the
correct canonical order. Removing `signature` and re-serializing compact is enough.

**2. Verify the Ed25519 signature**

```python
from cryptography.hazmat.primitives.asymmetric.ed25519 import Ed25519PublicKey

key_bytes = bytes.fromhex(r["signing_key_id"])
sig_bytes  = bytes.fromhex(sig_hex)

pub = Ed25519PublicKey.from_public_bytes(key_bytes)
pub.verify(sig_bytes, canonical)  # raises InvalidSignature on failure
print("signature OK")
```

Or with `PyNaCl`:

```python
import nacl.signing, nacl.encoding

vk = nacl.signing.VerifyKey(bytes.fromhex(r["signing_key_id"]))
vk.verify(canonical, bytes.fromhex(sig_hex))
print("signature OK")
```

**3. Check the input hash (optional)**

The `input_sha256` is the SHA-256 over all request messages serialized as compact
JSON and hashed in order (no separator between messages).

```python
import hashlib, json

messages = [{"role": "user", "content": "Hello"}]
h = hashlib.sha256()
for m in messages:
    h.update(json.dumps(m, separators=(",", ":")).encode())

assert h.hexdigest() == r["input_sha256"], "input hash mismatch"
```

**4. Check the output hash (optional)**

```python
output_text = "Hello! How can I help you?"  # the raw text from the API response
assert hashlib.sha256(output_text.encode()).hexdigest() == r["output_sha256"]
```

---

## Getting the node's public key

The signing key used by a Maranode node is stable across restarts. It is generated
on first start and stored in `<data_dir>/bundle_signing.key`. The corresponding
public key is at `bundle_signing.pub` (raw 32 bytes, hex encoded).

You can also read it from any receipt's `signing_key_id` field. If you trust the
node operator, pin the key from the first receipt you receive. If you do not trust
the operator, use a receipt from an audit bundle that was exported and sealed before
the inference you want to verify.

---

## Piping audit prove into verify

```
maranode audit prove <request_id> | maranode-verify
```

This is the typical workflow for auditing a specific inference after the fact.

---

## Limitations

- The receipt proves the node signed these bytes. It does not prove the node was
  actually air-gapped at the time, unless you also check the isolation attestation
  chain (`maranode verify network` and the audit log entries with event
  `binary_attested`).
- Input and output hashes are over the text content. They do not cover metadata
  such as HTTP headers or timestamps.
- The `model_sha256` is the hash of the file the node had at inference time. If the
  operator replaced the file after the fact, the hash will differ from what you see
  in the registry. To detect this, compare `model_sha256` with a known-good hash
  from the model's original source.
