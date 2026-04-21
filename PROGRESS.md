# Audit Application — Progress Log

Single source of truth for "where are we right now". Read this after `CLAUDE.md` at the start of every session. Update it at the end of every meaningful change — new decision, new file, new prototype, resolved open question, anything future-you would need to know without scanning the whole project.

Entries are reverse-chronological. Most recent first.

---

## Current state

**Phase**: Scaffold complete. Cross-platform Tauri 2 project under `app/` with Svelte 5 frontend and Rust backend. Database migrations for foundations + core-domain modules applied at launch. Application launches to a sidebar-shell UI with Dashboard / Clients / Engagements / Library / Settings routes. No real engagement flow yet.

**Repo / scaffolding**: Git initialised at project root. Toolchain installed (Rust 1.95 stable, Node 25). See `SETUP.md` for how to run.

**Documentation files**:
- `CLAUDE.md` — project instructions
- `PROGRESS.md` — this file (running state)
- `README.md` — human-facing overview
- `NOTES.md` — long-form design rationale
- `MODULES.md` — module map and architectural decisions
- `DATA_MODEL.md` — SQL-ish schema (currently covers Modules 1, 2, 3, 4, 5, 12)
- `SETUP.md` — first-run developer setup

**Code layout**:
- `app/` — Tauri project
  - `app/src/` — Svelte 5 + TypeScript + Vite frontend
  - `app/src-tauri/` — Rust backend
    - `app/src-tauri/src/commands/` — Tauri commands per module
    - `app/src-tauri/src/db/` — SQLite connection + 7 migrations
    - `app/src-tauri/src/crypto/` — AES-256-GCM, Argon2id, OS keychain
    - `app/src-tauri/src/models/` — Rust structs per module

**Immediate next up**:
1. **Auth flow** — first-run user/firm creation with Argon2id verifier and master-key wrap. Enables SQLCipher keying.
2. **First engagement creation** — exercises `SyncRecord`, `ChangeLog`, `ActivityLog` together. First real mutation path.
3. **User access review vertical slice** — the recommended first prototype module (exercises 10 of 13 modules).
4. **Extend `DATA_MODEL.md` to Modules 6–9** (Testing, Evidence, Findings, Working Papers).
5. **Library bundle format** — how a library version ships between releases.

---

## Decision log

### 2026-04-21 — Full dev scaffold committed

Cross-platform Tauri 2 project scaffolded at `/Users/simsbgang/Desktop/Audit_Application/app/`.

**Frontend stack**:
- **Svelte 5** (runes) + **TypeScript** + **Vite 5**. Chosen over React/Vue: smaller runtime, compiles away, closer mental model to vanilla HTML, single-file components match Simba's preference for readability over ceremony.
- Routing is a simple writable store (`lib/stores/router.ts`) — no router library. URL isn't involved since this is a desktop app, not a web app.
- Theme toggle via a single `html.dark` class flipping CSS custom properties — same pattern as Simba's writing site. Palette defined in `styles/tokens.css`.
- Font stack: Lora (serif, headings/body) + system sans (UI) + system monospace. One gold accent (`#A17817` light / `#D9BB6A` dark). No decorative icons. Aesthetic matches the writing site.
- One invoke-wrapper module (`lib/api/tauri.ts`) holds the typed contract between frontend and Rust commands — changes to command signatures go through one file.

**Backend stack**:
- **Rust 2021 edition**, `rusqlite 0.32` with `bundled-sqlcipher-vendored-openssl` so SQLCipher ships statically (no OS-level SQLite/OpenSSL dependency on any platform).
- `aes-gcm` + `argon2` + `rand` for file encryption and password-based key derivation.
- `keyring 3` for OS keychain integration — Keychain on macOS, DPAPI-backed Credential Manager on Windows, Secret Service on Linux. Cross-platform single API.
- `uuid` with `v7` feature (time-sortable UUIDs).
- `tracing` + `tracing-subscriber` for structured logging.
- `parking_lot::Mutex` for the DB connection lock (faster than std::sync, no poisoning).
- `thiserror 2` for the `AppError` enum, with a custom `Serialize` impl so errors cross the Tauri IPC boundary as strings for the frontend.

**Database**:
- Seven migrations in `app/src-tauri/src/db/migrations/`, run at launch inside a `SchemaMigration` tracking table:
  1. `0001_foundations` — Module 12 (Sync & Storage: `KeychainEntry`, `EncryptedBlob`, `SyncRecord`, `ChangeLog`, `ConflictResolution`)
  2. `0002_identity` — Module 1 (`Firm`, `User`, `Role`, `License`, plans, BYO-key) with 5 seeded built-in roles
  3. `0003_clients` — Module 2 with 8 seeded industries
  4. `0004_engagements` — Module 3 with 5 seeded engagement statuses
  5. `0005_systems` — Module 4 with 7 seeded system templates
  6. `0006_library` — Module 5 (risk/control/test procedure library, `FirmOverride`)
  7. `0007_activity_log` — cross-cutting `ActivityLog` (reviewer-facing audit trail)
- SQLCipher key wiring is commented out in `db/mod.rs`; the scaffold currently runs unencrypted so development is unblocked. Keying is the next step, gated on the auth flow.
- `PRAGMA foreign_keys = ON` and `journal_mode = WAL` set on connection open.

**Verified**:
- `npm install` clean (48 packages).
- `npm run build` produces a 48KB gzipped JS bundle (18KB gzipped).
- `svelte-check` passes (87 files, 0 errors, 0 warnings).
- `cargo check` compiles cleanly (0 errors, 0 warnings — dead-code warnings suppressed at crate level for scaffold).
- `cargo test` passes: crypto round-trip and KDF determinism smoke tests both green.

**What the app does when launched**:
- Opens a 1280×800 window titled "Audit Application" with `titleBarStyle: Overlay` for native macOS feel (no-op on Windows/Linux).
- Initialises the SQLite DB at the OS-standard app-data dir (`~/Library/Application Support/com.simba.auditapp/` on macOS, `%APPDATA%\com.simba.auditapp\` on Windows, `~/.local/share/com.simba.auditapp/` on Linux).
- Applies migrations, seeds built-in roles/industries/statuses/templates.
- Renders the sidebar-shell UI. Dashboard queries `ping` and `current_user`. Clients/Engagements/Library routes render empty-state cards since no data exists yet.

**What's deliberately not in the scaffold**:
- Authentication flow (Argon2id verifier + master-key unwrap + SQLCipher keying). Designed — not wired.
- Any create/update/delete mutation path. Read-only list endpoints only.
- AI provider dispatch (Module 11). Placeholder command file only.
- Library bundle loader — library tables exist but content loading is deferred until the bundle format is designed.
- Client portal (Module 10) — separate codebase, separate auth realm; not scaffolded yet.

### 2026-04-21 — First-pass data model drafted

Schema drafted for the foundation + core-domain modules in `DATA_MODEL.md`:

- **Module 12** (Sync & Storage): `SyncRecord`, `ChangeLog`, `ConflictResolution`, `EncryptedBlob`, `KeychainEntry`
- **Module 1** (Identity & Licensing): `Firm`, `User`, `Role`, `License`, `SubscriptionPlan`, `PrepaidBalance`, `BYOKeyConfig`
- **Module 2** (Client Management): `Client`, `ClientContact`, `Industry`, `ClientSettings`
- **Module 3** (Engagement Core): `Engagement`, `EngagementPeriod`, `EngagementTeam`, `EngagementScope`, `EngagementBudget`, `EngagementStatus`
- **Module 4** (System Inventory): `System`, `SystemTemplate`, `CustomSystem`
- **Module 5** (Risk & Control Library): `LibraryRisk`, `LibraryControl`, `TestProcedure`, `FrameworkMapping`, `ExpectedEvidenceChecklist`, `FirmOverride`
- **Cross-cutting**: `ActivityLog` (reviewer-facing audit trail, distinct from sync `ChangeLog`)

Decisions embedded in the schema:
- **UUID v7** as primary key format (time-sortable, index-friendly).
- **Timestamps as Unix epoch seconds, UTC.** Never local time.
- **Soft delete lives on `SyncRecord.deleted`**, not per-entity. One filter, one place to enforce.
- **Ciphertext is not in the DB** — `EncryptedBlob` stores only metadata; the encrypted bytes live as files on disk, referenced by path. Keeps the DB small and backups efficient.
- **`sha256_plaintext` on blobs** — computed before encryption. Enables deduplication and prior-year evidence matching despite differing nonces.
- **Per-engagement encryption key** — `Engagement.encryption_key_id` wraps under the user master. A leaver's access revoked by rotating in-flight engagement keys rather than re-encrypting everything.
- **`ChangeLog` vs `ActivityLog`** — distinct concepts. `ChangeLog` is row-level, machine-oriented, feeds sync/conflict. `ActivityLog` is human-oriented, engagement-scoped, feeds the reviewer UI.
- **`FirmOverride` keyed by `code + library_version`**, not row `id`. Overrides survive library updates; conflicts surface for firm review when the base entity changes.
- **Library entries versioned, not mutated.** New library version = new rows, prior rows marked `superseded_by`. Preserves the historical record implicit in past engagements.

Data-model-level open questions (now tracked in `DATA_MODEL.md`, not here): UUID crate choice, JSON validation strategy (CHECK vs application-layer), library bundle format, blob directory layout, FTS5 adoption timing.

### 2026-04-21 — Module map committed + six architectural decisions

Thirteen-module architecture committed (see `MODULES.md`). Answered all six module-level open questions:

1. **Working paper granularity**: all three formats supported — per-test, per-section (e.g. all Change Management tests in one WP), and custom grouping. Implementation: `WorkingPaper` is independent of `Test`; many-to-many via `WPTestLink`. Rationale: different firms have different conventions.
2. **Risk / Control library ownership**: dev-shipped baseline (COBIT 2019, NIST CSF, ISO 27001, PCI) with firm-level overrides layered on top. Industry standards land via library updates; firms customise via `FirmOverride` which survives updates.
3. **Engagement carry-forward**: **hybrid clone with `derived_from`** — new engagements clone prior methodology into fresh records, each carrying a pointer to its source and the library version it was based on. Snapshot with lineage. Preserves audit defensibility while enabling recurring-finding detection and opt-in library updates. (See `NOTES.md` for rationale vs pure linking or pure cloning.)
4. **Control ↔ System mapping**: `LibraryControl` declares applicable system types; defaults auto-populate when engagement is scoped; auditor can manually override per engagement without affecting library or other engagements.
5. **Evidence re-use year-to-year**: fresh upload preferred (supports audit independence). Prior-year evidence shown inline for reference. Auditor can mark an item as "unchanged from prior year" → prior-year `Evidence` is linked via `PriorYearEvidenceLink`; attestation recorded in `ActivityLog`. UI gives fresh upload slightly more prominence.
6. **Activity log retention**: forever within an engagement; append-only. On engagement close, archived alongside the engagement as an immutable encrypted bundle for regulator, peer review, and internal QA.

### 2026-04-21 — Pricing model and multi-provider support finalised

Three consumer paths agreed:
- **Subscription** (Simba's API, low margin, retention focus) — monthly tier with included quota
- **Prepaid / pay-as-you-go** (Simba's API, higher markup, smaller top-ups priced higher to absorb payment processor fees) — e.g. $10 pack ~20% markup, scaling down to ~8% at $500+
- **BYO-key** (user links own Claude / Gemini / OpenAI / Meta key) — Simba's account not used; value-add is rule-based query refinement (clarifying questions, structured templates) that cleans queries before dispatch to the user's own provider. Priced as a flat software licence.

Multi-provider LLM support from day one via an abstract provider interface. Adapters for Claude (default), OpenAI, Gemini, Meta/Llama, and local Ollama.

Efficiency techniques applied across all tiers: prompt caching, Batch API, model routing, redaction-before-dispatch.

Anthropic commercial API terms confirmed to permit embedding Claude in a paid product (Simba pays API cost, charges subscription, profits on margin). What's prohibited: sharing Simba's raw API key with users, pure passthrough resale with no product wrapper, cross-entity key sharing.

### 2026-04-21 — Core architecture decisions

- **Platform**: Tauri desktop app (Windows, macOS, Linux) + web client portal
- **Data storage**: SQLCipher-encrypted SQLite (per-engagement keys) + AES-256-GCM per-file attachments
- **Key management**: keys wrapped by OS keychain (Keychain / DPAPI / Secret Service); password derived via Argon2id; password never stored
- **Sync**: client-side encrypted blobs only; server never sees plaintext
- **Reports**: generated locally via python-docx / openpyxl / weasyprint — no cloud round-trip
- **AD / Entra access**: LDAP (ldap3), not PowerShell — portable across OSes
- **Anti-piracy**: hardware-bound licence keys, server-side crown jewels, Tauri/Rust compile. Sketched; full design deferred to post-MVP.

### 2026-04-21 — Automation philosophy

Rule-based first, classical ML second, local LLM third, hosted LLM last. Default to the lowest tier that solves the problem. Anomaly detection explicitly scoped to provided evidence (not pentesting — that line is never crossed).

### 2026-04-21 — Feature scope

Core differentiators identified:
- Pre-built control libraries (COBIT 2019, NIST CSF, ISO 27001, PCI)
- System-specific test packs with subtle UI guidance
- Messy-HR-data workflow (multi-source ingester, synthetic leaver list, backwards test, manager attestation via client portal)
- Per-client year-over-year recurring finding detection (cross-client deferred to phase 2, anonymised and opt-in)
- Statistical and judgemental sampling with recorded seed
- Structured working papers (replaces Excel narrative WPs)
- Full graph cross-referencing across risk / control / test / evidence / finding / action plan / follow-up

### 2026-04-21 — Aesthetic direction

macOS-inspired, editorial, restrained. SF Pro / Inter, warm near-white and near-black, one firm accent, 1px hairline rules, soft shadows, rounded corners. No emoji, no decorative icons. Echoes Simba's personal writing site.

### 2026-04-21 — Project environment set up

Documentation files created in `/Users/simsbgang/Desktop/Audit_Application/`:
- `CLAUDE.md` — project instructions for Claude cold reads
- `PROGRESS.md` — this file
- `README.md` — human-facing overview
- `NOTES.md` — long-form design rationale

Project-specific Claude skills directory (`.claude/skills/`) not yet created — will add when specific reusable workflows emerge.

---

## Open questions (to resolve before or during MVP)

- **Billing provider**: Stripe alone, Paystack for African payments, or both?
- **Licence server**: self-hosted by Simba, or managed (Keygen, Gumroad, Paddle)?
- **Data model**: sketch full schema up-front, or grow organically from the first module?
- **Client portal ↔ desktop app**: direct sync through Simba's server, or federated per-firm tenant?
- **Subscription quota behaviour on overage**: hard block, upgrade prompt, or metered overage billing?
- **Local LLM shipping**: ship with Ollama bundled as an optional feature, or document setup and let power users install themselves?
- **First prototype module**: user access review is the recommended vertical slice (exercises 10 of 13 modules). Confirm before scaffolding.

---

## Explicitly deferred (not now, possibly later)

- **Custom anti-piracy key-derivation scheme** (revisit post-MVP; the encryption algorithm itself will remain standard AES-256-GCM)
- **Cross-client recurring finding intelligence** (phase 2, after per-client is proven across multiple engagements)
- **Mobile companion app** (not in scope)
- **Audit frameworks beyond COBIT / NIST CSF / ISO 27001 / PCI** (add as client demand appears — SOC 2, HIPAA, RBZ-specific, etc.)
- **Cross-firm benchmarking / industry baselines** (requires large multi-firm dataset; far future)

---

## How to update this file

- Append new decisions to the top of the decision log with today's date
- Move any resolved open questions into the decision log entry that resolved them
- Move newly-surfaced questions into Open Questions
- Keep Current State accurate — it's the first thing a cold reader sees
- Keep entries tight: decision, reasoning, and any constraint or follow-up. Long-form rationale goes in `NOTES.md`.
