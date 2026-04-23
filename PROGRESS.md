# Audit Application — Progress Log

Single source of truth for "where are we right now". Read this after `CLAUDE.md` at the start of every session. Update it at the end of every meaningful change — new decision, new file, new prototype, resolved open question, anything future-you would need to know without scanning the whole project.

Entries are reverse-chronological. Most recent first.

---

## Current state

**Phase**: Authentication + encrypted DB live. First-run onboarding generates a random 256-bit master key, wraps it under an Argon2id-derived KEK, and persists the wrap to `identity.json`. Every subsequent sign-in unwraps the master key and opens the SQLCipher DB; logout drops the connection and re-locks the file. Library v0.1.0 baseline bundle ships with the binary, is Ed25519-verified on first open, and loads into the DB idempotently; the Library route browses it (list/detail across risks and controls, with framework / system-type / keyword filters). Application launches into an `AuthGate` → Shell pipeline with Dashboard / Clients / Engagements / Library / Settings routes.

**Repo / scaffolding**: Git initialised at project root. Toolchain installed (Rust 1.95 stable, Node 25). See `SETUP.md` for how to run.

**Documentation files**:
- `CLAUDE.md` — project instructions
- `PROGRESS.md` — this file (running state)
- `README.md` — human-facing overview
- `NOTES.md` — long-form design rationale
- `MODULES.md` — module map and architectural decisions
- `DATA_MODEL.md` — SQL-ish schema (currently covers Modules 1, 2, 3, 4, 5, 6, 7, 8, 9, 12)
- `SETUP.md` — first-run developer setup

**Code layout**:
- `app/` — Tauri project
  - `app/src/` — Svelte 5 + TypeScript + Vite frontend
  - `app/src-tauri/` — Rust backend
    - `app/src-tauri/src/auth/` — session state (`AuthState`) + on-disk identity vault (`keyvault`)
    - `app/src-tauri/src/commands/` — Tauri commands per module
    - `app/src-tauri/src/db/` — SQLCipher connection (`Option<Connection>`, open-with-key) + 7 migrations
    - `app/src-tauri/src/crypto/` — AES-256-GCM, Argon2id (KDF + password verifier), OS keychain
    - `app/src-tauri/src/models/` — Rust structs per module
    - `app/src-tauri/src/paths.rs` — `AppPaths` (app data dir + db path)

**On disk**:
- `{app_data_dir}/identity.json` — plaintext JSON holding login material: `argon2_hash`, `kek_salt`, `mk_nonce`, `mk_wrapped`. Must be plaintext because it is read *before* the DB is unlocked.
- `{app_data_dir}/audit.db` — SQLCipher-encrypted DB. Unreadable without the master key recovered from `identity.json` + correct password.

**Immediate next up**:

1. **User access review vertical slice** — the recommended first prototype module (exercises 10 of 13 modules). Schema is now drafted through Modules 6-9; this slice is next. Planned flow: scope a system → create a Test from library UAM-C-001 → upload AD export + HR leaver list as `DataImport`s → rule-based match flags terminated-but-active / dormant / orphan accounts → exceptions elevate to `Finding`s.
2. **Schema cleanup** — drop `User.argon2_hash` and `User.master_key_wrapped` (future migration). Authoritative copies live in `identity.json`.
3. **Password change** — rekey the wrapped master key under a new KEK. Pattern is already in place; needs a Settings UI + `auth_change_password` command.
4. **Zeroise master key in memory** — add `ZeroizeOnDrop` to the MK buffer before it reaches SQLCipher. Defensive; not a known leak.

**Library follow-ups** (do when they become load-bearing, not before):

- **Firm-override UI** — editing a library entry creates a `FirmOverride` row keyed by `(code, library_version)`. Browse is read-only today; overrides were deliberately out of scope for the baseline.
- **Library updates in-app** — today the baseline is compiled in. A later flow will let a firm drop a new `.json` + `.sig` pair into an inbox directory, verify, and install as a new version with `superseded_by` lineage. The loader already supports the upgrade path; only the pick-up-from-disk wiring is missing.
- **Broader framework coverage** — ISO 27001 and PCI DSS mappings come in v0.2.0. Baseline only ships COBIT 2019 + NIST CSF.

---

## Decision log

### 2026-04-24 — Data model extended to Modules 6–9

Schema drafted for the workflow core — Testing, Evidence, Findings, and Working Papers. Unblocks the access review vertical slice.

**Module 6 (Fieldwork & Testing)** — `EngagementRisk`, `EngagementControl`, `Test`, `SamplingPlan`, `Sample`, `TestResult`, `TestConclusion`, `Connector`, `DataImport`. Engagement-level clones carry both `derived_from` (library lineage) and `prior_engagement_*_id` (carry-forward lineage); walking either chain resolves "where did this come from?" unambiguously. One `EngagementControl` → many `Test`s, one per system in scope.

**Module 7 (Evidence)** — `Evidence`, `TestEvidenceLink`, `EvidenceTag`, `EvidenceProvenance`, `PBCRequest`, `PBCStatus`, `PriorYearEvidenceLink`. Provenance is append-only chain-of-custody for evidence that passes through OCR / extraction / redaction before it lands on a test. PBC status history is separate from `ActivityLog` because it drives the client-portal dashboard and overdue reminders.

**Module 8 (Findings)** — `Finding` (CCCER: Condition/Criteria/Cause/Effect/Recommendation as separate blobs for independent editing and LLM-drafting), `FindingTestResultLink`, `FindingSeverity`, `RootCauseTaxonomy`, `ManagementActionPlan`, `FollowUp`, `RecurringFindingLink`. `RecurringFindingLink.match_type` spans the automation ladder: `exact_control` (rule, confidence 1.0), `same_root_cause` (rule), `semantic` (sentence-transformer, auditor-confirmable).

**Module 9 (Working Papers)** — `WorkingPaper`, `WPSection`, `WPTestLink`, `ReviewNote`, `SignOff`. `WorkingPaper.wp_type` (`per_test` | `per_section` | `custom`) is advisory for the UI, not a DB rule — the three-format decision from `MODULES.md`. `SignOff` is immutable ledger; reopens create a new row, never edit history.

**Resolved** (moved out of Open questions): UUID format (v7 confirmed), FK enforcement (`PRAGMA foreign_keys = ON` in `db::open_with_key`), library bundle format (covered by yesterday's work).

**Still open**: JSON validation strategy, FTS5 (blocked on plaintext-at-rest trade-off for encrypted blob content), blob storage directory layout (proposed `{app_data_dir}/blobs/<eng_id>/<first-two-chars-of-blob-id>/<blob_id>.bin`).

**Deliberately not in this commit**: no new tables are migrated yet. Schema is design-only until the access review slice needs a concrete subset, at which point migration 0008 will carve out the tables actually exercised — not the full set. Avoids dead tables.

### 2026-04-24 — Library module landed

Library v0.1.0 ships with the binary, loads on first DB open, and is browsable from the Library route. Unblocks the access review slice — risks, controls, and test procedures now exist in-DB for any engagement to select from.

**Bundle format** (documented in `NOTES.md` under "Library bundle format"):
- Plain JSON payload + a separate `.sig` file containing a hex-encoded Ed25519 signature of the *raw bundle bytes*. Detached signature over the bytes avoids the canonical-JSON trap (no need to agree on key ordering or whitespace — we verify exactly what we read).
- Bundle is self-contained: risks, controls, test procedures with inline evidence checklists and inline framework mappings. Cross-references use human-authored `code` strings (e.g. `UAM-C-001` → `UAM-R-001`), not UUIDs — authors never invent UUIDs, the loader assigns them.
- One bundle per version. Version upgrades insert new rows and set `superseded_by` on prior rows sharing the same `code`.

**Signing tool** (`tools/sign-library-bundle/`, standalone Cargo crate, `publish = false`):
- Three subcommands: `keygen --out <dir>`, `sign --key <path> --bundle <path>`, `verify --pubkey <hex32> --bundle <path>`.
- Private key lives outside the repo at `~/.config/audit-app/signing/library.key`, chmod 0600 on Unix. Documented in `tools/sign-library-bundle/README.md`.
- Public key fingerprint: `0964b2228e5e45c67a8e7ae870a77d25b2b46f275bc0beaa65080414bc2237d9`. Baked into `app/src-tauri/src/library/verify.rs` as `LIBRARY_PUBLIC_KEY`. Rotating the key requires a recompile — intentional; key lifecycle tied to release cadence, not runtime configuration.

**Baseline v0.1.0** at `app/src-tauri/resources/library/v0.1.0.json{,.sig}`:
- 3 risks (UAM, CHG, BKP), 5 controls (UAM-C-001 through BKP-C-001), 5 test procedures, 5 inline evidence checklists, 10 framework mappings.
- Frameworks: COBIT 2019 + NIST CSF. ISO 27001 and PCI DSS deferred to v0.2.0.
- System types covered: `generic-erp` and `core-banking`.
- Real audit language — test steps reference live system evidence, evidence checklists are things an auditor would actually ask for.

**Loader** (`app/src-tauri/src/library/loader.rs`):
- Called from `db::open_with_key` after migrations. Embedded via `include_bytes!` / `include_str!` so the bundle is part of the binary, not a separate file to ship.
- Flow: verify signature → parse → idempotency check (`SELECT COUNT(*) FROM LibraryRisk WHERE library_version = ?1`) → transaction insert using `code → uuid` maps built during risk and control inserts → set `superseded_by` on prior versions sharing `code`.
- Three tests cover fresh install, repeated install (no-op), and tampered bundle (rejected).

**Commands** (`app/src-tauri/src/commands/library.rs`):
- `library_version`, `library_list_risks`, `library_list_controls`, `library_get_control(id)`.
- Filter all list queries on `superseded_by IS NULL` so callers always see the current version.
- `library_get_control` returns a `LibraryControlDetail` with related risks, framework mappings, and test procedures inline — one round-trip per detail view.

**UI** (`app/src/lib/routes/Library.svelte`):
- List/detail toggle with tabs for Controls / Risks. Filters: framework dropdown, system-type dropdown, keyword input. Detail view shows objective, description, pill row (type / frequency / system types), framework mappings, related risks, and test procedures with numbered steps plus evidence checklists.
- `prettySystemType()` maps bundle codes (`generic-erp`, `core-banking`) to display strings.

**Verified**:
- `cargo test` — 25 passing (5 new library tests).
- `svelte-check` — 91 files, 0 errors, 0 warnings.
- `npm run build` — 81.62 kB JS / 21.60 kB CSS.

**Deliberately out of scope**:
- **Firm overrides**. Schema (`FirmOverride` keyed by `code + library_version`) is already in place from migration 0006; UI comes later.
- **Updating library in-app**. Baseline is compiled in. A later flow will let firms drop a new signed bundle into a watched directory. Loader already handles the upgrade path.
- **Test procedure browsing as a standalone list**. Procedures are viewed nested under their control. Can promote to a top-level list if the UX demands it.

### 2026-04-21 — SQLCipher keying wired

The DB file is now unreadable at rest without the user's password. Ends the "scaffold DB is unencrypted" footnote from the earlier milestone.

**Two-keys design**:
- **Master key (MK)** — a random 32-byte key. This is what SQLCipher uses as the page encryption key.
- **Key-encryption key (KEK)** — derived on demand from the user's password + a per-user salt via Argon2id with `Params::default()` (m=19456 KiB, t=2, p=1). Used only to wrap/unwrap MK.
- On onboard: generate random MK, derive KEK from password, encrypt MK with AES-256-GCM, store `(kek_salt, mk_nonce, mk_wrapped)` in `identity.json`.
- On login: fetch identity, verify Argon2 hash, re-derive KEK, decrypt MK, `PRAGMA key` the DB with MK.
- On logout: drop the Connection — SQLCipher forgets MK, the file re-locks.

**Why a plaintext identity file and not a second encrypted DB**:
- Login must read the argon2 hash + wrapped MK *before* the main DB can be opened. They cannot live inside the encrypted DB they are used to unlock. A small plaintext JSON is the simplest container for pre-unlock metadata.
- Argon2id makes the hash safe to leave on disk — brute-forcing a strong password against it is intentionally expensive.
- Future multi-user-on-one-local-DB is trivial to support: append another entry to `identity.users`, each wrapping the *same* MK under its own password. The current UI still enforces single user.

**New code**:
- `src-tauri/src/auth/keyvault.rs` — `Identity` + `UserCredential` structs, `create_first_user()`, `unlock()`, `load/save/exists/find_by_email`. Binary fields are hex-encoded (hex is already a dep; no new base64 dependency).
- `src-tauri/src/paths.rs` — `AppPaths { app_data_dir, db_path }`. Resolved once in setup, Tauri-managed. Keeps command handlers from dragging in `AppHandle`.
- `src-tauri/src/db/mod.rs` — `DbState` now wraps `Mutex<Option<Connection>>` so the DB is genuinely closed between logins. Public API: `new`, `open_with_key(path, &[u8; 32])`, `close`, `with(&Connection)`, `with_mut(&mut Connection)`. The `open_with_key` flow runs `PRAGMA key = "x'...'"` before any other access, then does a test read against `sqlite_master` to surface a wrong-key failure as `AppError::Crypto("database key rejected")` instead of letting migrations hit a confusing parse error.

**Changed code**:
- `src-tauri/src/lib.rs` — no longer calls `db::initialise(app)` eagerly. Setup just creates the data dir and installs three managed states: `AppPaths`, an empty `DbState`, and an empty `AuthState`. The DB is opened on onboard/login and dropped on logout.
- `src-tauri/src/commands/auth.rs` — rewritten around keyvault + `AppPaths`. `auth_status` now gates on `keyvault::exists()`, not `User` row count (the DB isn't open yet at that point). `onboard` purges any stale unkeyed `audit.db` left over from the previous phase, generates the identity, opens the keyed DB, seeds the Firm + User, and only then persists `identity.json` (so a failed seed leaves no orphan identity). `login` loads identity, unlocks MK, opens the DB, updates `last_seen_at`. `logout` closes the DB and clears the session.
- `src-tauri/src/commands/{clients,engagements}.rs` — migrated from `db.conn.lock()` to `db.with(|conn| ...)`. Same query bodies.

**Verified**:
- `cargo test` — 11 passing, up from 5. New tests: `db::tests::keyed_db_migrations_and_roundtrip` (open a keyed DB, run migrations, insert, close, reopen with same key, read back), `db::tests::wrong_key_rejected`, `db::tests::with_closed_db_returns_unauthorised`, plus 3 keyvault roundtrip tests.
- `cargo check` — clean.
- `svelte-check` — 91 files, 0 errors, 0 warnings.

**Dev migration note**: Anyone who ran the earlier scaffold has an unencrypted `audit.db` at `{app_data_dir}/`. On the first run of this version, `onboard` detects the file (no `identity.json` exists, but `audit.db` does) and removes it before creating the new keyed DB. No dev data is at risk — the scaffold never held real data — but log lines note the purge.

**Deliberately out of scope here**:
- Password change / rekey. Pattern is straightforward: decrypt MK with old KEK, derive new KEK from new password, re-wrap. Not needed until users can actually change their passwords from the UI.
- Zeroising MK in memory. The `zeroize` crate would help but SQLCipher holds the key internally too; a separate pass to add `ZeroizeOnDrop` to buffers can come later.
- Dropping `User.argon2_hash` and `User.master_key_wrapped`. Still populated on onboard (satisfies `NOT NULL`); authoritative copies are in `identity.json`. A future migration will remove the columns.

### 2026-04-21 — Authentication flow wired

First real feature fill-in. Every mutation command from here on can recover a `Session` and write attributable rows.

**Backend**:
- `src-tauri/src/auth/mod.rs` — `AuthState` holds `parking_lot::Mutex<Option<Session>>`. Registered as Tauri-managed state in `lib::run`. `AuthState::require()` returns `AppError::Unauthorised` when the lock is empty — this becomes the gate for every mutation command.
- `src-tauri/src/crypto/password.rs` — Argon2id PHC-format hash + verify. Separate from `crypto::kdf` (which derives raw key bytes for file/DB encryption); this module only produces verifier strings that live in `User.argon2_hash`.
- `src-tauri/src/commands/auth.rs` — four commands:
  - `auth_status` — returns one of `onboarding_required` / `sign_in_required` / `signed_in { user }` based on `User` row count and current `AuthState`. The frontend picks the right screen from this.
  - `onboard` — validates input, hashes password (outside the DB lock — argon2 takes hundreds of ms), generates UUID v7 ids, inserts `Firm` + `User` in a transaction, assigns `role-partner`, sets the session.
  - `login` — fetches the user's stored hash, releases the DB lock, verifies outside the critical section, then updates `last_seen_at` and sets the session.
  - `logout` — clears the session.
- The `User.master_key_wrapped` column is populated with 60 random bytes as a placeholder. The SQLCipher keying step will replace this with the real wrapped key and remove the placeholder.
- Serde tagged enum `#[serde(tag = "kind", rename_all = "snake_case")]` produces `{kind: "onboarding_required"}` etc. on the wire. Matches the Svelte discriminated-union style on the frontend.

**Frontend**:
- `src/lib/stores/auth.ts` — `authView` writable store with four states (`loading` / `onboarding` / `sign_in` / `signed_in`). Exports `refreshAuth`, `onboard`, `login`, `logout` that call the typed API and transition the store.
- `src/lib/components/AuthGate.svelte` — mounts once, calls `refreshAuth()`, then routes to `Onboarding`, `SignIn`, or the app shell depending on state. Wrapped around `<Shell>` in `App.svelte`.
- `src/lib/routes/Onboarding.svelte` — firm name, country, display name, email, password. "Begin" creates the firm and signs the user in.
- `src/lib/routes/SignIn.svelte` — email + password. "Continue" verifies and signs in.
- `src/lib/components/Shell.svelte` — sidebar footer now shows the signed-in user (display name + email) with a "Sign out" button above the theme toggle.

**Design choices**:
- **No password recovery.** A recovery path would undermine the crypto since the password derives the DB/file keys. Recovery means wiping the local DB; sync recovery belongs to the sync/portal story, not here.
- **No cross-launch session persistence.** Every launch prompts sign-in. Safer for an audit tool — a recovered or shared device cannot silently impersonate.
- **Single user per local DB, for now.** The schema already supports multi-user in a firm (the sync story adds more users), but the onboarding UI enforces one. Multi-user provisioning comes later with roles + invites.
- **Session shape deliberately thin** — user id, firm id, display name, email. No role, no permissions: commands look those up per-call so stale cache can't grant privileges. The wrapped master key will be added here once SQLCipher keying lands.
- **Hash outside the DB lock, always.** Argon2 is intentionally slow; holding a mutex across it would serialise every concurrent command behind a login.

**Verified**:
- `cargo test` — 5 passing (crypto round-trip, KDF determinism, 3 Argon2 password tests).
- `cargo check` — 0 errors, 0 warnings.
- `svelte-check` — 91 files, 0 errors, 0 warnings.
- `npm run build` — 56KB JS / 12KB CSS, 326ms.

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
