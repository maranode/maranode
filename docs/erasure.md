# Crypto-Shredding and the Right to Erasure

This document describe how Maranode implements cryptographic erasure of workspace data, how it maps to GDPR Article 17, and what the limits are.

---

## What is crypto-shredding

Each workspace holds a data-encryption key (DEK). Every RAG chunk, document summary, and associated text stored for that workspace is encrypted with AES-256-GCM under the workspace DEK before it is written to disk.

Crypto-shredding means destroying the DEK. Once the key is gone, the ciphertext on disk is mathematically indistinguishable from random bytes. No decryption is possible.

This is different from record-level deletion. The ciphertext bytes may still exist in database pages, write-ahead logs, or backups, but they cannot be read.

---

## GDPR Article 17 mapping

Article 17 ("Right to erasure / right to be forgotten") requires that personal data be erased without undue delay when the data subject withdraws consent or the data is no longer necessary.

| Article 17 requirement | Maranode mechanism |
|---|---|
| Data erased without undue delay | `maranode workspace shred <id> --yes` destroys the DEK immediately |
| Erasure is irreversible | DEK is set to NULL in the workspace database; the wrapped DEK under the master KEK is also removed |
| Erasure is documented | A `workspace_shredded` event is written into the HMAC-chained audit log at the moment of shredding |
| Proof available for regulators | `maranode audit export-cert <id>` produces a plain-text deletion certificate |
| Key hierarchy does not allow recovery | DEKs are wrapped under a master KEK; when the DEK is destroyed, even the KEK cannot recover it |

The deletion certificate includes: workspace slug, timestamp, audit sequence number, actor, HMAC of the entry, and an erasure statement. The HMAC chain means any tampering with the certificate entry is detectable via `maranode audit verify`.

---

## How to shred a workspace

```bash
# destroy the DEK and write the deletion certificate to the audit log
maranode workspace shred my-workspace --yes

# export the certificate as a one-pager for a regulator or DPA
maranode audit export-cert my-workspace --output deletion_cert_my-workspace.txt
```

The certificate file looks like:

```
DELETION CERTIFICATE
====================

workspace : my-workspace
timestamp : 2025-03-12T14:22:01Z
audit seq : 1847
actor     : cli
hmac      : 9fa3e1...

statement :
  the encryption key (DEK) for workspace 'my-workspace' has been permanently
  destroyed. all data encrypted under this key is now cryptographically
  unreadable. this action cannot be reversed.

This certificate is derived from an HMAC-chained audit log entry.
verify integrity with: maranode audit verify
```

---

## What is erased

After shredding:

- RAG chunk text — unreadable (was encrypted with DEK)
- Document summaries — unreadable (was encrypted with DEK)
- Chunk metadata (document name, page numbers, chunk ids) — still readable; metadata was stored in plaintext
- Inference prompts and responses — only encrypted if `log_prompts = true` was set; if enabled they are also encrypted and unreadable; if not enabled they were never stored

---

## Caveats

**Already-exported plaintext.** If a user previously exported or received plaintext responses or document text (via API responses, CLI output, web UI), that data exists outside Maranode and is not affected by shredding. Crypto-shredding only covers data stored at rest inside Maranode's database.

**Backups.** If you have a backup of the workspace database taken before shredding, that backup still contains the encrypted ciphertext and possibly the wrapped DEK. The DEK in the live database is destroyed, but the backup is an independent copy. You should also delete or overwrite backups to fully satisfy an erasure request.

**Audit log.** The `workspace_shredded` event in the audit log is not erased. The audit log is an append-only integrity chain. The event does not contain any user data — it contains only the workspace slug, timestamp, and the erasure statement.

**Metadata.** Chunk metadata (document names, page numbers, source paths) is stored unencrypted. If this metadata constitutes personal data in your context, you need to also delete the workspace from the database with `DELETE FROM workspaces WHERE slug = '...'` or a future `maranode workspace delete` command.

**Parametric memory.** If inference was performed on personal data, the model weights are not affected. Maranode uses external GGUF models and does not fine-tune them; no personal data is written into model weights. This is a general LLM-deployment caveat, not specific to Maranode.

---

## Key hierarchy and recoverability

DEKs are wrapped under a master key-encryption key (KEK) stored at `<data-dir>/master.key`. When a DEK is shredded:

1. The DEK column in the workspace database is set to NULL.
2. The wrapped DEK value is gone.
3. Even with access to `master.key`, the DEK cannot be recovered because the wrapped value no longer exists.

The KEK itself is not destroyed by shredding. It remains in use for other workspaces. If you need to rotate the master key, use `WorkspaceDb::rotate_kek` (API) or a future `maranode workspace rotate-kek` command.

---

## Article 17 exceptions

Article 17(3) lists situations where erasure may be refused: exercise of freedom of expression, compliance with a legal obligation, public health, archiving in the public interest, or establishment/exercise/defence of legal claims.

Maranode does not enforce these exceptions automatically. If your deployment requires retention overrides, you should gate the `shred` command behind your own access control and approval workflow before exposing it to end users.
