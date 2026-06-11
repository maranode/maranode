## Low prioirty
- [ ] **Install script hosted at `get.maranode.com`** — The `install.sh` script exists in `scripts/` but needs to be deployed to the domain.
- [ ] **Multi-node deployment** — Leader election, shared model cache across nodes. Not started. Requires significant distributed systems work.
- [ ] **Graceful upgrades with no downtime** — No rolling upgrade mechanism. Restarts cause a brief gap. Needs socket activation (systemd) or a sidecar handoff mechanism.
- [ ] **Minimal Linux ISO** — Purpose-built OS image with Maranode as the only userspace. No shell, no SSH by default. Based on Alpine or a custom buildroot.

---

## Phase 3 — Differentiation (not started)

- [ ] **Seal audit HMAC key to TPM PCR** — Architecture says the Phase 3 appliance seals the audit key to a TPM PCR. Without this, an attacker with root can rewrite the audit log undetectably.

- [ ] **Multi-instance coordination without shared data** — Each Maranode instance stays isolated; federated queries route to the right instance without data crossing jurisdictions.
- [ ] **Shared model distribution** — Distribute model files to fleet members without sharing inference data.
- [ ] **Federated audit log aggregation** — Collect audit data across instances while maintaining per-instance isolation.
- [ ] **Immutable root filesystem** — OS partition is read-only; all mutable state on a separate partition.
- [ ] **Secure boot integration** — UEFI secure boot chain that verifies the kernel and initrd.
- [ ] **Pre-built VM image** — OVA / QCOW2 for air-gapped enterprise deployments.
- [ ] **Code Intelligence** — Code-aware chunking (by function/class rather than character count), syntax-aware search, code Q&A, basic vulnerability pattern scanning.
- [ ] **Custom fine-tuning workflow** — Fine-tuning pipeline that never sends data to external model trainers. Likely requires `llama.cpp` fine-tuning support or integration with a local fine-tuning tool.


## Phase 4 — Ecosystem (speculative)

- [ ] **Plugin/extension marketplace** — Sandboxed extensions for custom workflows. Architecture open question: WebAssembly or process isolation.
- [ ] **Vertical solutions** — Document intelligence for legal, code review for defense, clinical research for healthcare. Each as a separate paid product on top of Maranode.
- [ ] **Partner ecosystem / hardware bundle** — Integrators, MSPs, and hardware vendors.

---

## Cross-cutting / technical debt

- [ ] **Integration tests for auth flows** — OIDC/LDAP/SAML tests require external infrastructure; unit tests with mocks are missing.
- [ ] **TLS support** — The daemon listens on plain HTTP. Architecture doc says operators use a reverse proxy, but there should be a `--tls-cert` / `--tls-key` option for deployments without a proxy.
- [ ] **Audit log rotation** — The log file grows unboundedly. Rotation (by size or date) with integrity chain continuity across rotated files is not implemented.
- [ ] **Web UI accessibility** — No ARIA labels, no keyboard navigation audit, no screen reader testing.
- [ ] **Web UI mobile layout** — The UI assumes a wide viewport. No responsive breakpoints for narrow screens.
