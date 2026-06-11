- [ ] **Install script hosted at `get.maranode.com`** — The `install.sh` script exists in `scripts/` but needs to be deployed to the domain.

## Phase 2 — Commercial Foundation (remaining)

- [x] **Retention policy enforcement on schedule** — `prune_log` exists and is callable via API/CLI/UI, but there is no automated scheduler that runs it on a cron-like basis. Retention should be enforceable as a policy, not just a manual operation.
- [x] **Signed evidence bundles** — The ZIP bundle contains integrity data but the bundle itself is not cryptographically signed. Phase 2.3 roadmap says "cryptographically signed, timestamped." Needs cosign or a PGP/minisign step.

- [ ] **Multi-node deployment** — Leader election, shared model cache across nodes. Not started. Requires significant distributed systems work.
- [ ] **Backup and restore for audit logs** — No `maranode audit backup` / `restore` command. The files are plain JSON Lines so `rsync` works, but there is no documented or automated backup procedure.
- [ ] **Graceful upgrades with no downtime** — No rolling upgrade mechanism. Restarts cause a brief gap. Needs socket activation (systemd) or a sidecar handoff mechanism.

---

## Phase 3 — Differentiation (not started)

- [ ] **Remote verification endpoint** — `GET /v1/attestation/report` returns a signed attestation report that a third party can verify without trusting the operator.
- [ ] **Attestation reports as compliance artifacts** — Tie TPM report into the evidence bundle (Phase 2.3).
- [ ] **Seal audit HMAC key to TPM PCR** — Architecture says the Phase 3 appliance seals the audit key to a TPM PCR. Without this, an attacker with root can rewrite the audit log undetectably.

- [ ] **Multi-instance coordination without shared data** — Each Maranode instance stays isolated; federated queries route to the right instance without data crossing jurisdictions.
- [ ] **Shared model distribution** — Distribute model files to fleet members without sharing inference data.
- [ ] **Federated audit log aggregation** — Collect audit data across instances while maintaining per-instance isolation.

- [ ] **Minimal Linux ISO** — Purpose-built OS image with Maranode as the only userspace. No shell, no SSH by default. Based on Alpine or a custom buildroot.
- [ ] **Immutable root filesystem** — OS partition is read-only; all mutable state on a separate partition.
- [ ] **Secure boot integration** — UEFI secure boot chain that verifies the kernel and initrd.
- [ ] **Pre-built VM image** — OVA / QCOW2 for air-gapped enterprise deployments.

- [ ] **Code Intelligence** — Code-aware chunking (by function/class rather than character count), syntax-aware search, code Q&A, basic vulnerability pattern scanning.
- [ ] **Custom fine-tuning workflow** — Fine-tuning pipeline that never sends data to external model trainers. Likely requires `llama.cpp` fine-tuning support or integration with a local fine-tuning tool.
- [ ] **OCR for scanned PDFs** — Currently scanned image PDFs are rejected. Integration with `ocrmypdf` or a Tesseract binding would make the document pipeline handle real-world PDF archives.
- [ ] **Table extraction** — Detect and extract tables from PDFs as structured Markdown. The current pipeline extracts plain text only.
- [ ] **Re-summarize on demand** — API/UI endpoint to regenerate a document summary if the model has changed or the auto-summary was skipped at ingest time.




## Phase 4 — Ecosystem (speculative)

- [ ] **Plugin/extension marketplace** — Sandboxed extensions for custom workflows. Architecture open question: WebAssembly or process isolation.
- [ ] **Vertical solutions** — Document intelligence for legal, code review for defense, clinical research for healthcare. Each as a separate paid product on top of Maranode.
- [ ] **Partner ecosystem / hardware bundle** — Integrators, MSPs, and hardware vendors.

---

## Cross-cutting / technical debt

- [ ] **Token count estimation** — The context overflow guard in `chat.rs` uses `chars ÷ 3.5 ≈ tokens` heuristic. A proper tokenizer (or llama.cpp's `llama_tokenize`) would give exact counts and allow setting `max_tokens` based on the actual model context window.
- [ ] **Model context window metadata** — `ModelManifest` does not store the model's context window size. This is needed for the token budget guard and for surfacing to users.
- [ ] **Integration tests for auth flows** — OIDC/LDAP/SAML tests require external infrastructure; unit tests with mocks are missing.
- [ ] **API rate limiting per IP** — The rate limiter is per-workspace. There is no per-IP rate limiting for the auth endpoints, making brute-force attacks on `/v1/auth/login` possible.
- [ ] **TLS support** — The daemon listens on plain HTTP. Architecture doc says operators use a reverse proxy, but there should be a `--tls-cert` / `--tls-key` option for deployments without a proxy.
- [ ] **`/v1/models` pagination** — No pagination on model list; could be slow with large model stores.
- [ ] **Audit log rotation** — The log file grows unboundedly. Rotation (by size or date) with integrity chain continuity across rotated files is not implemented.
- [ ] **Web UI accessibility** — No ARIA labels, no keyboard navigation audit, no screen reader testing.
- [ ] **Web UI mobile layout** — The UI assumes a wide viewport. No responsive breakpoints for narrow screens.
