# Maranode Roadmap

This is a working document. It only tracks what remains to be built. Completed work is summarised in the section below and documented in full in `HANDBOOK.md`.

The structure follows the open-core model: build adoption with the open source Core, then layer Enterprise capabilities on top once we have validated demand.

---


## What is already shipped

The full inventory is in `HANDBOOK.md`. The short version:

- Local inference (CPU, CUDA, Metal, ROCm, Vulkan) with OpenAI-compatible API
- Model store, pull from Hugging Face, content-addressed blobs, SHA-256 verification
- Air-gap enforcement via iptables, periodic self-probe, fail-closed on drift
- Append-only HMAC-chained audit log, size/age rotation into sealed compressed segments, compliance exports (GDPR/HIPAA/SOC 2/ISO 27001), evidence bundles, SIEM forward
- Proof-carrying inference: signed receipts, reproducible deterministic mode, receipt replay
- TPM 2.0 PCR read, binary self-hash at start, key sealing, attestation reports, TEE detection
- Workspaces with per-workspace quotas, system prompts, audit segments, crypto-shred
- Local user accounts, API-key auth, OIDC, SAML SSO (basic), session management, per-IP rate limiting
- Fully local RAG: embeddings, cosine retrieval, cited answers, collections, PDF/OCR/table extraction, encrypted store
- Incident response: declare, investigate, snapshot, break-glass
- Data classification engine with collection-level labels and policy enforcement
- CLI (`maranode` + `maranoded`), web UI, systemd service, Docker images, package repositories (apt, dnf, pacman, Homebrew)
- Benchmark tool, integration and e2e test suite, threat model, proof demo

---

## What We Will Not Do

A roadmap is more useful when it says no to things. These are deliberately excluded:

- **Hosted cloud service.** Inference-as-a-service contradicts the value proposition.
- **General-purpose operating system.** The Phase 3 appliance is purpose-built and minimal; we are not competing with Ubuntu.
- **Model training infrastructure.** Maranode is inference-focused. Training is a different product category.
- **Mobile applications.** Possibly a remote control app for a Maranode instance, but no inference on phones.
- **Proprietary model formats.** We use GGUF and other open standards.
- **Cryptocurrency, NFTs, or blockchain anything.** None of these solve problems Maranode has.

---

## Phase 1 — Close the Gaps 

These are items that are partially built or missing from an otherwise mature runtime. They block calling the Core feature set complete.

### Hardware

- **OpenVINO engine (Intel NPU)** — build scaffolding and Dockerfile exist; the actual inference engine integration is not wired up. Needs the OpenVINO C++ runtime linked and the inference path plumbed through.
- **AMD Ryzen AI / XDNA NPU** — same state as OpenVINO. Scaffolding present, no inference path yet.

### Trust & Distribution

- **Signed releases** — binaries and packages need Sigstore / cosign signatures so users can verify they are running what we published.
- **Reproducible build verification** — the script is in `scripts/`; it needs an independent run confirming the binary matches source before we can publish the claim.
- **Third-party security audit** — one external firm, published report. Until this is done we can make assertions but cannot prove them to an auditor.

### Operations

- **Prometheus metrics** — local-only export, no scraping outside the host. Needed for self-hosted ops dashboards.
- **Native TLS** — the daemon currently listens on plain HTTP and expects a reverse proxy. Direct TLS with certificate management should be built in for deployments that cannot run a proxy.

### Web UI

- **Remaining gaps in the browser UI** — several API capabilities (generation parameters, workspace advanced settings) were recently added but polish and edge-case handling is still in progress.

**Exit criteria:**
- Both NPU backends have passing benchmark numbers on real hardware
- Signed release artifacts are available and the cosign verification command is documented
- Security audit report is published
- Prometheus endpoint ships and is documented

---

## Phase 2 — Enterprise Completeness

The building blocks for a paying enterprise customer are largely present. The remaining gaps are the ones a procurement or security team will ask about.

### Access & Identity

- **Linux namespace isolation enforcement** — workspace network namespaces have full lifecycle (create, delete, exist check) but inference requests are not yet routed through the namespace. The enforcement step is missing.
- **LDAP / Active Directory group sync** — login against an LDAP directory works; group membership sync and group-to-workspace mapping are not built yet.
- **Role-based access control (RBAC)** — fine-grained roles beyond "admin / workspace key holder". Needed before any serious multi-team deployment.

### Infrastructure

- **Multi-node deployment with leader election** — single-node only today. Clustering is the primary blocker for high-availability production use.
- **Shared model cache across nodes** — each node holds its own copy; a cluster should share the blob store.
- **Zero-downtime upgrades** — restart causes a brief gap. Rolling upgrade support requires the multi-node work above.

**Exit criteria:**
- Maranode Enterprise is generally available
- First paying customer
- RBAC and SSO work end-to-end and are documented
- At least one customer is running in a multi-node configuration

---

## Phase 3 — Differentiation

These are the capabilities that make Maranode structurally hard to replicate. By this point, larger players may have noticed the project.

### Federated Deployment

Multiple Maranode instances coordinating without sharing data. The target customer is a cross-jurisdiction enterprise where data must stay inside each country. Specific deliverables:
- Instance discovery and trust establishment without a central authority
- Shared model distribution without shared inference
- Audit log aggregation with maintained per-instance isolation

### Hardened Appliance

A minimal Linux distribution as an ISO or pre-built VM image. Maranode runs on it with no other userspace. This is where the original KlazOS work gets folded in. Sold as an Enterprise add-on for defense, intelligence, and healthcare networks.

### Specialized Inference

- **Code intelligence** — parsing, refactoring assistance, vulnerability scanning. Enterprise tier.
- **Code-aware RAG chunking** — chunk by function and symbol boundaries rather than token windows. Core.
- **Local fine-tuning workflow** — a fine-tuning path that never sends data to model trainers. Enterprise.

**Exit criteria:**
- 10+ Enterprise customers
- $1M+ ARR
- One named reference customer in a regulated industry with a published case study

---

## Phase 4 — Ecosystem 

Speculative. Depends entirely on what we learn in earlier phases. The likely directions:

- **Vertical solutions:** Document intelligence for legal, code review for defense contractors, clinical research for healthcare. Each becomes a separate paid product on top of Maranode.
- **Partner ecosystem:** Integrators, MSPs, and hardware vendors who bundle Maranode.
- **Plugin/extension marketplace:** Sandboxed extensions for custom workflows. Open ecosystem but vetted (think VS Code marketplace).
- **Compliance-as-a-service:** Help customers turn Maranode audit logs into regulatory submissions.

We will not commit to any of these now. The question to answer first is: what does the customer ask for repeatedly that we cannot already provide?

---

