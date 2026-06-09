# Proof-carrying inference — demo

The smallest end-to-end version of MOAT-ROADMAP item 0.1. 
An answer that comes with a signed receipt a stranger can
verify offline, and that catches any later tampering — using only `openssl` and
`sha256sum`, never trusting Maranode.

This is a demo, not the product. Token generation is stubbed so it runs with no
GPU and no model download. The cryptography and the verification are real.

## Run it

```bash
cd demos/proof-carrying-inference
./demo.sh # the full test
./demo.sh --replay # plus the reproducible-inference beat
```

Needs `bash`, `openssl` (1.1.1 or newer — on macOS use the brew openssl, not the
system LibreSSL), and `sha256sum` or `shasum`. Set `DEMO_PAUSE=0` to remove the
pacing pauses when recording.

## What you see (the 60 seconds)

1. The daemon makes an ed25519 key once. Only its public half is published.
2. A model and a prompt.
3. The inference runs and the daemon emits a **receipt** — a small signed JSON
   binding model hash + decode settings + input hash + output hash + time + key.
4. A *stranger* verifies it with `verify-receipt.sh`. No Maranode involved. All
   checks pass:

   ```
     [ PASS ] key id matches the published key (1697230b694c79a4)
     [ PASS ] signature valid — the record is authentic and unmodified
     [ PASS ] input matches the receipt
     [ PASS ] output matches the receipt
     [ PASS ] model matches the receipt
     VERIFIED
   ```

5. (`--replay`) Re-running the inference produces byte-identical output, so the
   decision can be replayed, not only asserted.
6. Someone edits one dose in the saved answer. Re-verify: the signature is still
   valid, but the output no longer matches its hash. **Caught.**
7. The attacker tries to cover it by rewriting the receipt to match. They cannot
   re-sign without the private key, so the signature fails. **Caught.**

That is the whole pitch in one screen: *tokens you can prove.*

## Verify it without our scripts

The point is that you do not have to trust `verify-receipt.sh` either. After a
run, the artifacts are in `work/`. Check them by hand:

```bash
# 1. the signature covers the exact receipt body
openssl base64 -d -A -in work/receipt.sig > /tmp/sig.bin
openssl pkeyutl -verify -pubin -inkey work/daemon.pub \
  -rawin -in work/receipt.body.json -sigfile /tmp/sig.bin
# -> Signature Verified Successfully

# 2. the saved output really hashes to what the receipt claims
sha256sum work/output.txt
grep -o '"output_sha256":"[^"]*"' work/receipt.body.json
# -> the two hashes match (or they do not, and you know it was changed)
```

Standard tools, no custom verifier. This is the same principle as the rest of
Maranode: each layer can be checked with tools you already trust.

## What this proves, and what it does not

Be precise about this — it is half the value.

**It proves:**
- **Authenticity and integrity.** The holder of the daemon key stated that this
  model, under these settings, turned this input into this output at this time.
  Anyone with the public key can confirm the record is unaltered and who signed
  it, offline.
- **Tamper-evidence.** Any change to the output, the prompt, the model, or the
  receipt itself is detected.
- **Replayability** (with `--replay`, and in the real runtime through item 0.2).
  Greedy decoding lets a holder of the model re-derive the same bytes, which
  turns an assertion into something independently reproducible.

**It does not prove:**
- **Trustless correctness of the computation.** This is an attestation *signed by
  the key holder*, not a zero-knowledge proof that the model really computed the
  output. A party who holds the private key could sign a false triple. Replaying
  the model closes this when you have the model; a fully trustless proof
  (the CommitLLM / TOPLOC / ZK research line) is a different, much heavier thing
  and is explicitly out of scope here.
- **Anything about who holds the key.** A receipt is only as meaningful as the
  custody of the signing key. In the real runtime the key is sealed to the TPM
  (roadmap item 6) and the daemon runs fail-closed, so a receipt means "this
  machine running this software signed it," not merely "someone signed it."

In short: this defends against post-hoc tampering and forgery by anyone without
the private key. It does not, on its own, defend against a compromised signer —
that is what TPM-sealing, fail-closed operation, and reproducibility are for.

## How it maps to the real feature (MOAT-ROADMAP 0.1)

| step | in this demo | in the runtime |
|---|---|---|
| `receipt-schema` | the body JSON | versioned, with behavioral hash + optional TPM PCR |
| `daemon-signing-key` | ed25519 keypair | sealed to the TPM, with rotation |
| `receipt-emit` | `sign-receipt.sh` | emitted on the API response behind a flag |
| `receipt-in-chain` | — | written into the HMAC audit chain |
| `verifier-standalone` | `verify-receipt.sh` + the openssl steps above | a single no-dependency verifier binary |
| `audit-prove-cmd` | — | `maranode audit prove <record-id>` |

To bind a *real* model output instead of the stub, replace `run-inference.sh`
with a call to `/v1/chat/completions` and pass the real GGUF path to
`sign-receipt.sh`. Nothing else changes.

## Files

- `demo.sh` — runs the whole story.
- `sign-receipt.sh` — the daemon side: build and sign a receipt.
- `verify-receipt.sh` — the stranger side: standalone, openssl + sha256sum only.
- `run-inference.sh` — stubbed inference (deterministic).
- `_lib.sh` — shared helpers.
- `work/` — generated artifacts, git-ignored.
