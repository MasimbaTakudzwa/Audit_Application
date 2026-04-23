# Audit Application — Progress Log

Single source of truth for "where are we right now". Read this after `CLAUDE.md` at the start of every session. Update it at the end of every meaningful change — new decision, new file, new prototype, resolved open question, anything future-you would need to know without scanning the whole project.

Entries are reverse-chronological. Most recent first.

---

## Current state

**Phase**: First real vertical slice live — User Access Review (Module 6/7/8 ribbon) — plus Evidence (Module 7) browsable chain of custody. From an engagement detail page an auditor can add a library control (clones risks + tests into the engagement), upload an AD export and an HR leavers list as encrypted `DataImport`s, run a rule-based matcher — `UAM-T-001` (terminated-but-active) or `UAM-T-003` (dormant accounts) — to produce a `TestResult`, elevate exceptions to a draft `Finding` with severity, and see every raw upload, matcher report, and free-form attachment in an Evidence table with source, test linkage, and one-click download. All mutations are attributable (SyncRecord + ChangeLog + ActivityLog + EvidenceProvenance) and cross-firm isolated.

Earlier foundations remain: authentication + encrypted DB, Ed25519-verified library baseline (now v0.1.0 + v0.2.0), Clients / Engagements CRUD, and the Library browse route.

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

1. **More rule matchers** — change-management (`CHG-T-001`) and backup (`BKP-T-001`) are the next two to ship. Same dispatcher-through-rule-variant pattern as the two UAR rules. Orphan-accounts (AD rows with no matching HR master) is the other natural UAR extension.
2. **CCCER-shaped finding editor** — split the flat condition/recommendation fields into condition / criteria / cause / effect / recommendation. The Finding table and `update_finding` command already carry enough data; this is a UI + model extension.
3. **Working paper view** — one card per test, sidebar navigation grouping by control, review-note-as-margin-annotation pattern.
4. **Schema cleanup** — drop `User.argon2_hash` and `User.master_key_wrapped` (future migration). Authoritative copies live in `identity.json`.
5. **Password change UI** — the `change_password` command is already registered; Settings needs a form that calls it.
6. **Zeroise master key in memory** — add `ZeroizeOnDrop` to the MK buffer before it reaches SQLCipher. Defensive; not a known leak.

**Library follow-ups** (do when they become load-bearing, not before):

- **Firm-override UI** — editing a library entry creates a `FirmOverride` row keyed by `(code, library_version)`. Browse is read-only today; overrides were deliberately out of scope for the baseline.
- **Library updates in-app** — today the baseline is compiled in. A later flow will let a firm drop a new `.json` + `.sig` pair into an inbox directory, verify, and install as a new version with `superseded_by` lineage. The loader already supports the upgrade path; only the pick-up-from-disk wiring is missing.
- **Broader framework coverage** — ISO 27001 and PCI DSS mappings remain deferred. v0.2.0 carries COBIT 2019 + NIST CSF forward; ISO/PCI land in a later bundle.

---

## Decision log

### 2026-04-24 — Evidence module (Module 7, minimal subset)

Every raw upload, matcher report, and free-form attachment is now tracked as an Evidence row with an append-only provenance chain, browsable from the engagement detail page. This is what unlocks the rest of the auditor workflow — a finding that doesn't cite the evidence behind it is useless for review.

**Migration 0009** (`app/src-tauri/src/db/migrations/0009_evidence.sql`):
- `Evidence` is the browsable face of an `EncryptedBlob`. One-to-one with a blob, many-to-one with an engagement; optionally linked to a `Test`, a specific `TestResult`, an `EngagementControl`, and/or a `DataImport`. The `source` column carries intent (`auditor_upload` / `data_import` / `matcher_report` / `client_portal` / `prior_year_link`) so reviewers can filter by provenance at a glance.
- `TestEvidenceLink` lets a single Evidence row back several tests without duplication — `relevance` is `primary` / `supporting` / `cross_reference`, default `supporting`. The primary linkage stays on `Evidence.test_id`.
- `FindingEvidenceLink` is the citation edge from a finding to the evidence that supports it.
- `EvidenceProvenance` is an append-only chain of custody keyed on `(evidence_id, chain_ordinal)`. `chain_ordinal` starts at 1 (origin) and increments for every transformation (OCR, extraction, redaction, prior-year reuse). Actor type is `user` | `system` | `portal_user`. `detail_json` carries rule-specific metadata (filename, purpose_tag, exception_count, etc.).
- **Deferred**: `PBCRequest`, `PBCStatus`, `EvidenceTag`, `PriorYearEvidenceLink`. They land when the flows that exercise them do (client portal, prior-year reuse, tagging UI).

**Rust (`app/src-tauri/src/commands/evidence.rs`)**:
- `persist_evidence(&tx, NewEvidence, actor_user_id, now) -> AppResult<String>` is the one-stop helper for inserting Evidence + origin EvidenceProvenance + SyncRecord + whole-row ChangeLog. Takes `&Transaction<'_>` so it composes cleanly inside `upload_data_import`, `run_access_review`, and the free-form `upload_evidence` flow without nested transactions.
- `link_evidence_to_test(&tx, test_id, evidence_id, relevance, now) -> AppResult<bool>` is idempotent (returns `false` when the link already exists) so replaying a matcher run doesn't duplicate links.
- Tauri commands: `engagement_list_evidence`, `engagement_upload_evidence`, `engagement_download_evidence`, `finding_attach_evidence`, `finding_detach_evidence`, `finding_list_evidence`. All enforce the engagement → client → firm_id chain against the session firm. Uploads require the unlocked master key (via `auth.require_keyed()`); list endpoints only require a session.
- 25 MB upload cap mirrors the DataImport cap. Ciphertext is written via the existing `blobs::write_engagement_blob` path; the Evidence row carries `blob_id` alongside the auditor-facing metadata.

**Auto-created evidence hooks in `commands/testing.rs`**:
- `upload_data_import` now persists an Evidence row immediately after the DataImport insert: `source = data_import`, `test_id = NULL` (engagement-level), `data_import_id` set, `provenance_action = "data_import"`, `actor_type = "user"`. The raw upload is browsable from day one; a matcher run later attaches it to its test as supporting evidence.
- `run_access_review` now persists *three* things after its TestResult insert: (a) a `matcher_report` Evidence row wrapping the JSON report blob, linked to both the Test and the specific TestResult; (b) a `TestEvidenceLink` from the Test to the AD DataImport's Evidence row; (c) the same link from the Test to the leavers DataImport's Evidence row when the rule consumed one. `actor_type = "system"` on the matcher-report provenance entry — the run is automated, not an auditor action.
- `TestRow` gained `engagement_control_id` so the matcher-report Evidence row can carry the denormalised control id without a second query.

**UI (`app/src/lib/routes/EngagementDetail.svelte`)**:
- New Evidence section at the bottom of the engagement page. Lists title, source label, filename, linked test code, size, and obtained-at for every row. Download is a one-click action per row that calls `engagement_download_evidence`, assembles a `Blob` from the returned `Uint8Array`, and triggers a browser download with the original filename preserved.
- Upload form supports title, description, obtained-from, optional test link, and file. Tests populate the test dropdown from the already-loaded tests list.
- `submitUpload` (data-import flow) and `runMatcher` both now refresh the Evidence list alongside their own primary data — the auto-created rows appear without the auditor having to refresh.

**TypeScript (`app/src/lib/api/tauri.ts`)**:
- New types: `EvidenceSummary`, `EvidencePayload`, `UploadEvidenceInput`, `EvidenceLinkInput`.
- New bindings: `engagementListEvidence`, `engagementUploadEvidence`, `engagementDownloadEvidence`, `findingAttachEvidence`, `findingDetachEvidence`, `findingListEvidence`.

**Verified**:
- `cargo test --lib commands::evidence` — five new tests: auto-Evidence on DataImport upload, matcher run creates matcher-report Evidence + links both DataImports, attach/detach finding roundtrip (idempotent), free-form upload persists + decrypts, cross-firm listing rejected with `NotFound`.
- `cargo check --all-targets` clean.

**Deliberately deferred**:
- Attach-evidence-to-finding UI. The backend commands are wired and tested, but the finding editor doesn't surface them yet — that lands with the CCCER editor so we don't rework the same rows twice.
- Provenance viewer. The chain is written; no UI reads it yet. Shows up when OCR or extraction actions start appending chain entries beyond the origin row.
- Evidence tagging, PBC requests, prior-year reuse. All deferred to their own flows.

**Files of note**: `app/src-tauri/src/db/migrations/0009_evidence.sql`, `app/src-tauri/src/db/migrations/mod.rs`, `app/src-tauri/src/commands/evidence.rs`, `app/src-tauri/src/commands/testing.rs`, `app/src-tauri/src/lib.rs`, `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`.

### 2026-04-24 — Dormant-accounts matcher + library v0.2.0

Second UAR rule ships. Same testing flow as the terminated-but-active rule, different matcher behind the same dispatch point.

**Matcher** (`app/src-tauri/src/matcher/access_review.rs`):
- `run_dormant_accounts(ad, as_of_secs, threshold_days)` flags AD rows whose last-sign-in is older than the threshold (default 90 days) or `"Never"`/`"0"` sentinels. Rows with no last-logon column value are skipped; unparseable values are counted and reported separately so the auditor can see data-quality noise.
- `parse_last_logon()` handles six formats without pulling in a date crate: Windows FILETIME (17-19 digit ticks since 1601), Unix epoch seconds (9-10 digits), Unix epoch millis (13 digits), ISO 8601 date (`YYYY-MM-DD`), ISO 8601 datetime with `T` or space separator, and the `"Never"`/`"0"` sentinels. Uses Howard Hinnant's `days_from_civil` to avoid dragging in `chrono`/`time` for one conversion.
- Returns a `DormantReport` with `ad_rows_considered`, `ad_rows_skipped_disabled`, `ad_rows_skipped_no_last_logon`, `ad_rows_skipped_unparseable`, `threshold_days`, `as_of_secs`, plus `DormantException` rows recording the original AD ordinal, the parsed timestamp, and the days-since calculation.

**Library v0.2.0** (`app/src-tauri/resources/library/v0.2.0.json{,.sig}`):
- Carries forward all v0.1.0 content (3 risks, 5 controls, 5 test procedures) plus one new procedure: `UAM-T-003` "Review of dormant application accounts" under `UAM-C-002`. `sampling_default: "none"` because the rule runs against the full AD population.
- Same Ed25519 signing flow as v0.1.0. Bundle signed with the private key at `~/.config/audit-app/signing/library.key`, `.sig` committed alongside the JSON.
- `loader::install_baseline_bundles` now installs v0.1.0 then v0.2.0 in sequence. v0.1.0 rows that share a `code` with v0.2.0 rows get `superseded_by` set, so `WHERE superseded_by IS NULL` returns the current set (3 + 3 risks, 5 + 5 controls, 5 + 6 test procedures). The duplication is intentional — it exercises the upgrade path on every fresh install, giving us confidence the superseding logic works before we ship a real v0.3.0.

**Rule dispatch** (`commands/testing.rs`):
- Introduced `AccessReviewRule` enum with `TerminatedButActive` and `DormantAccounts` variants plus `for_test_code(&str) -> Option<Self>`. `run_access_review` now branches on the test's `code`: the terminated rule resolves a leavers import; the dormant rule doesn't need one. A test code that has no matching rule returns `AppError::Message` rather than silently running the wrong matcher.
- `AccessReviewRunResult` refactored to honestly represent both rules: `leavers_import_id` / `leavers_import_filename` / `leaver_rows_considered` / `ad_rows_skipped_unmatchable` are now `Option`, and the struct gains `rule`, `ad_rows_skipped_no_last_logon`, `ad_rows_skipped_unparseable`, `dormancy_threshold_days`.
- `RuleOutcome` intermediate struct carries everything downstream (TestResult insert, ChangeLog, ActivityLog, tracing) needs regardless of which rule ran. Keeps the persistence layer rule-agnostic.

**UI** (`app/src/lib/routes/EngagementDetail.svelte`):
- `MATCHER_ENABLED_CODES = new Set(["UAM-T-001", "UAM-T-003"])` replaces the previous `startsWith("UAM-T-")` check. `UAM-T-002` (manual access-review completeness test) no longer renders a "Run matcher" button it couldn't honour.
- `AccessReviewRunResult` TypeScript interface updated to mirror the new Rust shape; fields render defensively when null.

**Verified**:
- `cargo test --lib` — 67 passing (up from 53). New tests: seven in `matcher::access_review::tests` covering threshold flagging, Windows FILETIME + `"Never"` sentinel, missing last-logon column vs missing value, unparseable counting, ISO 8601 parsing; two in `commands::testing::tests` covering dormant-rule happy path (no leavers needed, 2 exceptions from stale timestamps) and rejection when `run_access_review` is called against a test code with no rule (`CHG-T-001`).
- `svelte-check` — 92 files, 0 errors, 0 warnings.
- `clone_library_control_shares_risks_between_sibling_controls` asserts `test_count == 3` now — cloning `UAM-C-002` with v0.2.0 applied produces two `Test` rows (`UAM-T-002` and `UAM-T-003`).

**Deliberately deferred**:
- No UI for overriding the 90-day threshold or the `as_of` date. Backend threshold is a `u32` parameter; the command currently hardcodes the default and `as_of = SystemTime::now()`. A per-engagement policy config will plug in when a second firm asks for a different window.
- Orphan-account rule (AD rows with no matching HR master) is the next rule in this module. Same shape — add a variant, add a matcher, add a library test procedure.

**Files of note**: `app/src-tauri/src/matcher/access_review.rs`, `app/src-tauri/src/commands/testing.rs`, `app/src-tauri/src/library/loader.rs`, `app/src-tauri/resources/library/v0.2.0.json{,.sig}`, `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`.

### 2026-04-24 — User access review vertical slice shipped

First end-to-end feature landed. From the Engagement detail page an auditor can now:

1. Add a library control (`UAM-C-001` etc.) — clones library risks, the control, and its test procedures into engagement-scoped rows with `derived_from` + `library_version` lineage.
2. Upload an AD export and an HR leavers list tagged by purpose. Files encrypt with AES-256-GCM under a per-engagement content key; ciphertext lives on disk at `{app_data}/blobs/{eng_id}/{aa}/{blob_id}.bin` **without** the auth tag (tag stays in `EncryptedBlob` for tamper detection), row metadata in `DataImport`.
3. Run the rule-based `UAM-T-001` matcher. CSV is parsed header-canonicalised (BOM-tolerant, quoted commas), joined email-first with a logon-name fallback, and AD rows explicitly marked disabled are skipped. Exceptions + the serialised report land as a `TestResult` blob; the `Test.status` advances to `in_review`.
4. Elevate any exception result to a `Finding`. Codes are per-engagement sequential (`F-001`, `F-002`, …), severity defaults to `sev-medium`, condition text is machine-drafted from the matcher report, the triggering `TestResult` is linked via `FindingTestResultLink`. Pass results and cross-firm `test_result_id`s are rejected.

Every mutation writes a `SyncRecord` + whole-row `ChangeLog` + `ActivityLog` row. Every read and mutation enforces the engagement → client → firm_id chain against the session firm. 53 backend tests, 0 svelte-check errors.

**Migration 0008** seeds the five-level `FindingSeverity` table (`sev-critical` / `sev-high` / `sev-medium` / `sev-low` / `sev-observation`). Only the tables actually exercised by this slice were migrated — `Evidence`, `ManagementActionPlan`, `ReviewNote`, `WorkingPaper` remain design-only.

**Deliberately minimal in this first cut**:
- Matcher runs with no UI override for AD/leavers selection. Backend accepts override ids; frontend just sends `null` and the command picks the newest matching purpose. Override UI will appear when auditors start wanting to re-run against an older file.
- Finding editor is read-only. Elevation produces a draft row with generic condition + recommendation text; inline editing + CCCER breakdown is next.
- Evidence is not yet attached to findings. The `EncryptedBlob` the matcher writes is referenced from the `TestResult`, not surfaced as a browsable `Evidence` row.

**Files of note**: `app/src-tauri/src/matcher/{csv,access_review}.rs`, `app/src-tauri/src/commands/{testing,findings}.rs`, `app/src-tauri/src/blobs/mod.rs`, `app/src-tauri/src/db/migrations/0008_testing_findings.sql`, `app/src/lib/routes/EngagementDetail.svelte`.

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
