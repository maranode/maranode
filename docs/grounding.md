# Grounding Proof for RAG

When Maranode answers a question using RAG (retrieval-augmented generation), it can produce a cryptographic proof that binds the answer to the exact source chunks that were retrieved. This document explains what is proven, what is not, and how to verify it.

---

## What grounding proves

Every inference receipt contains a `sources` array when RAG was used. Each entry records:

| Field | Meaning |
|---|---|
| `chunk_id` | UUID of the chunk in the RAG store |
| `doc_id` | UUID of the source document |
| `source` | File path or URL the document came from |
| `doc_sha256` | SHA-256 of the full document text at ingest time |
| `chunk_hash` | SHA-256 of the chunk text at ingest time |
| `score` | Cosine similarity score at retrieval time |

The receipt also has:

- `grounded: true` — at least one RAG source was retrieved above the configured `min_score` threshold
- `grounded: false` — the model answered from parametric memory only

The full receipt is Ed25519-signed with the daemon's signing key, so it cannot be forged or altered.

---

## What you can verify

**At retrieval time**: The receipt records which chunks influenced the answer and what their scores were. If you have the receipt, you know exactly what context the model saw.

**After the fact (tamper detection)**: The `chunk_hash` in each source ref is the SHA-256 of the chunk text at ingest time. If a source document was modified or re-ingested with different content after inference, the hash will no longer match. You can check this with:

```bash
maranode audit verify-sources <request_id>
```

This command opens the RAG store, re-hashes each referenced chunk, and reports any mismatch:

```
  ✓ chunk a3f1b2c0 (contracts/nda.pdf) — hash OK
  ✓ chunk 7e90d41a (contracts/nda.pdf) — hash OK
  ✓ chunk 12bc8f33 (policies/data-retention.md) — hash OK

✓ All 3 source(s) verified — no tampering detected.
```

If tampering is detected:

```
  ✗ chunk a3f1b2c0 (contracts/nda.pdf) — TAMPERED
    receipt:  9fa3e1...
    stored:   9fa3e1...
    computed: 44b7cc...   ← stored text no longer matches what was ingested

✗ Source verification FAILED.
```

---

## Parametric leakage limit

Grounding proves that the retrieved chunks were used as context. It does not and cannot prove that the model's answer was *derived only* from those chunks.

Large language models have parametric memory: knowledge baked into the weights during training. When an answer is grounded in retrieved sources, the model may still blend in parametric knowledge to fill gaps, rephrase, or add related facts. This is a fundamental property of transformer-based models and is not specific to Maranode.

Maranode does not claim that `grounded: true` means the answer is solely derived from the retrieved chunks. It means those chunks were present in the context window at generation time, and the receipt proves it.

If you require answers to be strictly limited to source content, you must enforce that through the system prompt and use the receipt as evidence that the constraint was applied — not as a proof of the output's scope.

---

## Verify a receipt manually

The `sources` array in the receipt JSON lets you verify chunk hashes outside of Maranode:

```python
import hashlib, json

with open("receipt.json") as f:
    receipt = json.load(f)

for src in receipt["sources"]:
    # read the chunk text from your own copy of the document
    chunk_text = ...  # the exact text from the source chunk
    computed = hashlib.sha256(chunk_text.encode()).hexdigest()
    match = computed == src["chunk_hash"]
    print(f"{src['chunk_id'][:8]}: {'OK' if match else 'MISMATCH'}")
```

The `doc_sha256` is the SHA-256 of the full document text at ingest time; verify it the same way to check the entire document.

---

## Summary table

| Claim | Supported | Notes |
|---|---|---|
| These chunks were in the context window | Yes | Signed in the receipt |
| The chunk text has not changed since ingest | Detectable | `audit verify-sources` re-hashes live store |
| The source document has not changed | Detectable | `doc_sha256` field in receipt |
| The answer was derived only from these chunks | No | Parametric leakage; fundamental LLM limit |
| The retrieval scores are accurate | Yes | Cosine similarity at retrieval time, signed |
