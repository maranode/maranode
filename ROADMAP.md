# Maranode Roadmap

This roadmap describes the phased development plan for Maranode from pre-alpha to commercial maturity. It is a working document and will change as we learn from users.

The structure follows the open-core model: build adoption with the open source Core, then layer Enterprise capabilities on top once we have validated demand.

---

## Guiding Principles

Before the phases, the rules we apply to every decision:

1. **Ship the runtime first, the platform later.** Every feature must answer: does this help someone deploy local AI today, or is it a fantasy of what we wish we needed?
2. **Open source decisions are irreversible.** Anything we publish under Apache 2.0 stays in Core forever. We choose deliberately.
3. **Evidence over claims.** Every security or privacy assertion must be verifiable by the user, not just trusted.
4. **Boring technology where possible.** llama.cpp, SQLite, iptables, systemd. We innovate on integration, not on reinventing infrastructure.
5. **No telemetry, ever.** Not even "anonymous" usage statistics. The product makes a promise; we have to keep it.

---

## Phase 0 — Foundation (Current)

**Goal:** A runnable local AI runtime that demonstrably stays local. One person should be able to install it, run a model, and prove to themselves that nothing leaked.

**Timeline:** ~3 months from start.

**Deliverables:**
- Core daemon (Rust) exposing OpenAI-compatible HTTP API
- llama.cpp integration via FFI for CPU inference
- Model store with content-addressed blob storage and SHA-256 verification
- iptables-based air-gap enforcement with one-command toggle
- Append-only audit log with HMAC chain integrity
- CLI: `maranode serve`, `maranode model pull/list/remove`, `maranode chat`, `maranode audit verify`
- Single binary distribution (no Docker required, but Docker image also published)
- Install script: `curl -sSL get.maranode.com | sh` — supports Ubuntu, Debian, RHEL/Fedora, Alpine, Arch
- Minimal documentation: install, basic usage, security model

**Explicitly out of scope:**
- NPU support (Phase 1)
- Web UI (Phase 1)
- Multi-user (Phase 2)
- Clustering (Phase 2)

**Exit criteria:**
- Three independent users (outside the founding team) successfully install Maranode on three different Linux distributions and run inference.
- Independent verification that no outbound network traffic occurs after model download.
- Audit log integrity check passes on a corrupted log (i.e., we detect the tampering).

---

## Phase 1 — Adoption (3-9 months in)

**Goal:** Make Maranode the obvious choice for engineers who care about local AI isolation. Build the open source community.

**Timeline:** 6 months.

### Phase 1.1 — Performance & Hardware Support
- NPU acceleration via OpenVINO (Intel Core Ultra)
- AMD Ryzen AI support (XDNA driver integration)
- Apple Silicon support via Metal (macOS deployment for developers; not the production target but useful for adoption)
- Benchmark suite comparing CPU, NPU, GPU paths
- Model quantization tools integrated (Q4_K_M default recommendation)

### Phase 1.2 — User Experience
- Web UI (Tauri-packaged or browser-based, undecided) for chat, model management, audit log viewing
- Improved CLI with progress bars, color output, helpful errors
- Configuration file system (`/etc/maranode/config.toml`) with sensible defaults
- Health check endpoint and basic Prometheus metrics (local only, not exported)

### Phase 1.3 — Trust & Distribution
- Reproducible builds (verifiable that binary matches source)
- Signed releases (Sigstore / cosign)
- Package repositories: apt, dnf, pacman, Homebrew
- Independent security audit (third-party firm)
- Publication of threat model and audit results

**Exit criteria:**
- 1,000+ GitHub stars
- 100+ active installations (we count this via voluntary self-reports, not telemetry)
- 10+ external contributors
- One public reference deployment (a person or organization willing to talk about using it)
- Security audit report published

---

## Phase 2 — Commercial Foundation (9-18 months in)

**Goal:** Launch Maranode Enterprise. Generate first commercial revenue. Validate which features actually matter to paying customers.

**Timeline:** 9 months.

This phase is where business model meets technical decisions. The features below are the ones we believe enterprises will need based on the open-core playbook from HashiCorp, GitLab, and Sentry. We will adjust based on actual customer conversations.

### Phase 2.1 — Multi-tenancy (Core)
- Workspace concept: isolated environments with separate model stores and audit logs
- Per-workspace resource quotas (memory, model count, inference rate) ✓
- Linux namespace isolation under the hood ✓ (lifecycle scaffolding; enforcement pending)
- *In Core* because individual users benefit from separating contexts

### Phase 2.2 — Identity & Access (Enterprise)
- Local user database (Core)
- SSO via SAML 2.0 (Enterprise)
- LDAP/Active Directory integration (Enterprise)
- OIDC for modern identity providers (Enterprise)
- Role-based access control with fine-grained permissions (Enterprise)

### Phase 2.3 — Compliance Tooling (Enterprise)
- Pre-built audit log exports formatted for:
  - GDPR Article 30 records of processing
  - HIPAA access logs
  - SOC 2 evidence collection
  - ISO 27001 documentation
- Customizable retention policies
- Cryptographic evidence bundles (signed, timestamped)
- *Not in Core* because building these requires significant ongoing legal expertise

### Phase 2.4 — Operations (Enterprise)
- Multi-node deployment with leader election
- Shared model cache across nodes
- Graceful upgrades with no downtime
- Backup and restore tooling for audit logs
- *Not in Core* because operational complexity is what enterprises pay to outsource

**Exit criteria:**
- Maranode Enterprise generally available
- First paying customer
- Documented pricing model
- Sales motion defined (likely product-led with sales-assist for >$50K deals)

---

## Phase 3 — Differentiation (18-30 months in)

**Goal:** Build the capabilities that make Maranode structurally hard to replicate. By this point, larger players may have noticed the open source project. The moat needs to be real.

**Timeline:** 12 months.

### Phase 3.1 — Remote Attestation
- TPM-based attestation of runtime integrity ✓ (binary SHA-256 + TPM 2.0 PCR read via direct device I/O; `maranode verify attest`; `BinaryAttested` audit event at startup)
- Remote verification that a deployment is unmodified
- Hardware root of trust integration
- "Attestation reports" as a compliance artifact ✓ (JSON report with binary hash, PCR values, audit chain status, self-hash)

This is the feature that separates "we say it's local" from "you can cryptographically verify it's local." It is technically demanding but it is the strongest possible answer to an auditor.

### Phase 3.2 — Federated Deployment
- Multiple Maranode instances coordinating without sharing data
- Useful for cross-jurisdiction enterprises (data stays in each country)
- Shared model distribution without shared inference
- Audit log aggregation with maintained isolation

### Phase 3.3 — Hardened Appliance
- This is where the original KlazOS work gets folded in
- A minimal Linux distribution shipped as an ISO or pre-built VM image
- Maranode runs on it with no other userspace
- Sold as Enterprise add-on for the highest-security customers (defense, intelligence, healthcare networks)

### Phase 3.4 — Specialized Inference
- Document intelligence pipeline (PDF → text → embeddings → retrieval) as a Core feature
  - **Update:** the retrieval base shipped early — the optional `maranode-rag`
    crate provides local embeddings, a SQLite vector store, and grounded chat
    with citations (text ingestion today; PDF/markdown extraction still to come).
- Code intelligence (parsing, refactoring, vulnerability scanning) — likely Enterprise
- Custom fine-tuning workflow that never sends data to model trainers — Enterprise

**Exit criteria:**
- 10+ Enterprise customers
- $1M+ ARR
- One named reference customer in a regulated industry
- Published case study showing measurable compliance outcome

---

## Phase 4 — Ecosystem (30+ months in)

This is speculative and depends entirely on what we learn in earlier phases. The likely directions:

- **Vertical solutions:** Document intelligence for legal, code review for defense contractors, clinical research for healthcare. Each becomes a separate paid product on top of Maranode.
- **Partner ecosystem:** Integrators, MSPs, and hardware vendors who bundle Maranode.
- **Plugin/extension marketplace:** Sandboxed extensions for custom workflows. Open ecosystem but vetted (think VS Code marketplace).
- **Compliance-as-a-service:** Help customers turn Maranode audit logs into regulatory submissions.

We will not commit to any of these now. The question to answer first is: what does the customer ask for repeatedly that we cannot already provide?

---

## What We Will Not Do

A roadmap is more useful when it says no to things. These are deliberately excluded:

- **Hosted cloud service.** Inference-as-a-service contradicts the value proposition.
- **General-purpose operating system.** The Phase 3 appliance is purpose-built and minimal; we are not competing with Ubuntu.
- **Model training infrastructure.** Maranode is inference-focused. Training is a different product category.
- **Mobile applications.** Possibly a remote control app for an Maranode instance, but no inference on phones.
- **Proprietary model formats.** We use GGUF and other open standards.
- **Cryptocurrency, NFTs, or blockchain anything.** None of these solve problems Maranode has.

---

## Versioning

Maranode follows semantic versioning starting from v1.0 (when we declare general availability):
- **0.x.y:** Pre-alpha. Anything can change.
- **1.0.0:** First stable release. API stability guaranteed for Core HTTP API.
- **1.x.y:** Backwards-compatible features.
- **2.0.0:** Breaking changes (which we will avoid until necessary).

