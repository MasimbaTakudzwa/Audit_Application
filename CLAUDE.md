# Audit Application — IT audit platform + client document portal

Cowork, read this file at the start of every session. It captures the current design and decisions so you don't have to scan the whole project to get oriented. For live state ("what's done, what's in flight, what's next"), read `PROGRESS.md` immediately after this.

## What this is

A commercial IT audit application targeting small-to-mid audit firms in Africa (and eventually beyond), built by Simba (IT Auditor at Baker Tilly Zimbabwe). Two sides to the product:

- **Auditor desktop app** (paid) — where the actual audit work happens: scoping, risk assessment, ITGC and ITAC testing, working papers, findings, management action plans, reports.
- **Client web portal** (free) — a Huddle-like document-sharing space where clients upload evidence, respond to attestations, and exchange files with the audit team. Simple, browser-accessible, no install.

The auditor side is the value. The client portal is table stakes — the thing that gets clients to engage with the platform at all.

## Market and constraints

- **Primary market**: audit firms in Zimbabwe and wider Africa, where TeamMate, Workiva, and AuditBoard are too expensive or too cloud-dependent.
- **Bandwidth reality**: client environments often have poor internet. The desktop app must be fully usable offline; only the client-facing portal needs reliable connectivity.
- **Data sensitivity**: every engagement touches confidential client data. Encryption at rest and in transit is non-negotiable.
- **Hardware reality**: most auditors have mid-range corporate laptops. Don't require GPUs or high RAM for core features.
- **Recordkeeping reality**: many client organisations are paper-based or disorganised. The app must tolerate and normalise messy inputs (CSV, Excel, scanned PDFs, photos of paper, email exports), not assume clean data.

## Tech decisions (so far)

- **Desktop app**: **Tauri** (Rust core + web frontend) for cross-platform (Windows, macOS, Linux). Small installer (~10MB), fast, meaningfully harder to reverse than Electron/JS bundles.
- **Local data**: **SQLite** with **SQLCipher** (AES-256) for the engagement database. Per-engagement encryption keys so compromise of one engagement does not leak another.
- **File attachments**: **AES-256-GCM** per-file before write. Keys wrapped by OS keychain (Keychain on macOS, DPAPI on Windows, Secret Service on Linux).
- **Key derivation**: **Argon2id** from user password. Password itself is never stored.
- **Sync**: client-side encrypted blobs pushed to the project's cloud backend when online. Server never sees plaintext — limits breach blast radius and is a trust/marketing point.
- **Reports**: generated locally (templates → python-docx / openpyxl / weasyprint → Word / Excel / PDF). No cloud round-trip for output.
- **AD / Entra access**: via **LDAP** (ldap3 library) not PowerShell — works identically on all three OSes.
- **Anti-piracy**: Tauri/Rust compile, hardware-bound licence keys signed by a private key held by Simba, periodic online re-validation with offline grace, crown-jewel logic (cross-firm intelligence, prompt templates, control library updates) held server-side. Custom *key derivation* scheme tying keys to user + hardware + licence — **custom algorithm design is explicitly NOT a goal**. Don't roll your own crypto.

## Automation tiers — rule-based first, AI last

Most audit automation is deterministic, not AI. Default to the lowest-tier option that solves the problem.

1. **Rule-based scripts** (free, fast, offline, zero leak risk):
   - User access reconciliation (JOIN AD export against HR leaver list, flag terminated-but-active, dormant, SoD conflicts)
   - Statistical sampling (MUS, attribute sampling per ISA 530 / AICPA) with recorded seed
   - ITAC tests (duplicates, boundary, reconciliation, rounding, completeness, Benford's Law)
   - Cross-referencing (risk → control → test → evidence → finding → action plan → follow-up) via foreign keys
2. **Classical ML** (free, runs on CPU, offline):
   - Anomaly detection (IsolationForest, LocalOutlierFactor) on client-provided data
   - Semantic finding matching via `sentence-transformers` (`all-MiniLM-L6-v2`, ~80MB)
   - Keyword / topic extraction (YAKE, spaCy)
   - OCR (Tesseract, with OpenCV preprocessing for bad scans)
3. **Local LLM** (optional, for firms with capable hardware):
   - Ollama + small model (Phi-3 Mini, Llama 3.2 3B, Gemma 2B) via llama.cpp
   - Useful for narrow classification or summarisation tasks. Not a default.
4. **Hosted LLM** (paid tier, server-side):
   - Drafting finding write-ups, extracting controls from policy PDFs, summarising interviews
   - Claude as default (Haiku for most, Sonnet for complex cases)
   - Multi-provider support (see below) via an abstract provider interface

**Boundary**: the app analyses evidence clients provide. It does not probe client systems. No port scans, no credential testing, no exploits. Anomaly detection is pattern analysis on exports, not pentesting.

**Hallucination control**: all LLM output is presented as a *suggested draft* that the auditor must review and accept. Never auto-commit LLM text into a working paper. Source evidence is shown inline for fast review.

## Pricing model — three consumer paths

Users choose one of three LLM access models at sign-up, and can switch later.

### 1. Subscription (low margin, retention focus)

- Queries go through Simba's Anthropic / OpenAI / Gemini / Meta account
- Monthly tiers with included token or operation quotas
- Token usage monitored per user; overage behaviour: block, upgrade prompt, or metered billing (TBD)
- Lowest per-query cost — deliberately cheap to encourage commitment and long-term retention

### 2. Prepaid / pay-as-you-go (higher markup)

- Queries go through Simba's account, billed per top-up pack
- Markup higher than subscription (by design — pushes users toward subscription)
- **Small top-ups cost more per unit** because payment-processing fees eat margin on tiny transactions. Approximate shape: $10 pack ~20% markup, $50 pack ~15%, $250 pack ~10%, $500+ pack ~8%.
- Good for occasional users and firms not ready to commit

### 3. BYO-key — Bring Your Own API key (lowest Simba-side cost, flat software fee)

- User links their own API key from Claude, Gemini, OpenAI, Meta (Llama hosted), or other supported providers
- Simba's account is not used for query dispatch
- Value-add: **rule-based query refinement** — form-driven UI and deterministic templates clean and structure queries *before* dispatch to the user's own provider. This is NOT an LLM call through Simba. The app asks clarifying questions, normalises the query, bundles relevant context (engagement metadata, prior findings, control references), then sends the cleaned query to the user's provider.
- Result: the user's own API calls are shorter, better-structured, and therefore cheaper than if they called the provider directly, even though they pay their own token costs.
- Good for enterprise firms with existing LLM contracts, strict data-handling rules, or dedicated capacity
- Priced as a flat software licence (per seat or per engagement)

### Efficiency techniques (applied everywhere)

Across all three tiers, to reduce cost for Simba and users alike:

- **Prompt caching** (90% discount on cached tokens with Claude — control library, engagement context, prior findings are highly cacheable)
- **Batch API** (50% discount for non-interactive jobs — overnight summarisation of working papers in an engagement)
- **Model routing** — short classification to cheap / small models, complex drafting to larger models, rules wherever possible before touching an LLM
- **Redaction before dispatch** — client names, account numbers, IDs regex-stripped; LLM sees anonymised version; real values swapped back on return

## Multi-provider support

From day one, the LLM layer is an **abstract provider interface**. Adapters planned for:

- Anthropic Claude (default; supports prompt caching, Batch API)
- OpenAI (GPT-4.1 / GPT-5 family)
- Google Gemini
- Meta Llama (via hosted providers — Groq, Together, Fireworks)
- Local Ollama (self-hosted, for BYO-compute firms)

User selects default provider in settings. App tracks per-provider cost and usage. Graceful fallback if one is rate-limited. Prompt differences (system prompt handling, tool-use formats) normalised inside the adapter.

## What makes the auditor side attractive (beyond the Huddle layer)

- **Pre-built control libraries** mapped to COBIT 2019, NIST CSF, ISO 27001, PCI — reusable across clients and engagements
- **System-specific test packs** (SAP S/4HANA, Oracle EBS, Dynamics 365, core banking, payroll, ecommerce, in-house apps) — auditor picks the system, relevant procedures appear as options
- **Custom system builder** for in-house client apps — define control points (input / processing / output / interfaces), app scaffolds test templates
- **Subtle UI guidance**: collapsible side panels with "things commonly missed", "what good evidence looks like", "typical client pushback and how to respond" per procedure. Inline hints when an answer looks thin on a high-risk control. Dismissable, suggestive — never blocking.
- **Expected-evidence checklists** per item, dev-shipped defaults plus auditor-extensible additions
- **Messy HR data workflow**: multi-source ingester (CSV, Excel, OCR'd PDF, photos of paper forms, email exports, payroll, badge logs), synthetic leaver list with preserved provenance, backwards test (AD disabled accounts → ask management to confirm reason), manager attestation flow via the client portal
- **Recurring finding detection** — per-client year-over-year first; cross-client / firm-level intelligence later (anonymised, opt-in, firm-tenant only)
- **Sampling engine** — statistical or judgemental, recorded seed for reviewer reproducibility, automatic error projection to population
- **Cross-referencing** — navigable graph across risk / control / test / evidence / finding / action plan / follow-up. Replaces manual Excel WP references.
- **Working papers** — structured forms (not free-form cells), one test per card, sidebar navigation, review notes as inline margin annotations that clear with a tick, audit trail preserved
- **Export** to branded Word / PDF for anything leaving the app

## Aesthetic — macOS-inspired, editorial, restrained

- UI typography: SF Pro or Inter; body copy in the same family (no mixing serifs unless for branding)
- Generous whitespace, 1px hairline rules over heavy borders
- Soft shadows only on elevated cards; rounded corners 8–12px
- Restrained palette: warm near-white background, near-black text, one firm accent colour; dark mode with an equivalent palette
- Status pills (Draft / Prepared / In Review / Reviewed / Locked) with subtle colour coding
- No decorative iconography. Icons only where they aid recognition (status, file types, actions).
- No emoji in UI or content, ever.

This echoes Simba's personal writing site — same family of restraint.

## Voice for content and UI copy

- Direct imperative for UI strings ("Add evidence", not "Click here to add evidence!")
- British spelling: "authorise", "organised", "colour", "analyse", "recognised"
- No marketing adjectives. No "powerful", "seamless", "unlock", "revolutionary", "next-gen".
- No exclamation marks.
- Error messages state what went wrong and what to do — no apologies, no blame.
- Help text explains audit context the user needs, not the UI mechanics they can already see.

## Things to avoid

- **Don't roll your own crypto.** Use AES-256-GCM, Argon2id, TLS 1.3. Custom work is in *key derivation*, not algorithm design.
- **Don't require cloud for core work.** The desktop app must be fully useful offline.
- **Don't cross the pentesting line.** The app analyses client-provided evidence. It does not touch client systems directly.
- **Don't auto-commit LLM output.** All AI output is a *suggested draft* — the auditor reviews and accepts before it enters a working paper.
- **Don't ship a feature behind an LLM call when a deterministic alternative exists.** Rules first, ML second, LLM last.
- **Don't add emoji or decorative iconography.** This is a professional tool, not a consumer app.
- **Don't add trackers, analytics in user workspaces, or dark patterns.** Firm-level opt-in usage analytics are fine; silent user telemetry is not.
- **Don't share API keys across unrelated organisations.** Each firm has its own account and billing scope.

## Useful file pointers

- `CLAUDE.md` — this file (project instructions for cold reads)
- `PROGRESS.md` — running log of decisions, current state, and next steps. Update at the end of every meaningful change.
- `README.md` — human-readable project overview
- `NOTES.md` — long-form design rationale for decisions that didn't fit here
- `.claude/skills/` — project-specific Claude skills (empty for now; add as reusable workflows emerge)
