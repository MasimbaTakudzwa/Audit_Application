# Audit Application — Progress Log

Single source of truth for "where are we right now". Read this after `CLAUDE.md` at the start of every session. Update it at the end of every meaningful change — new decision, new file, new prototype, resolved open question, anything future-you would need to know without scanning the whole project.

Entries are reverse-chronological. Most recent first.

---

## Current state

**Phase**: Pivot from rule-catalogue building to design-philosophy thickness — the matcher work covers eleven rules across four ITGC/ITAC families and demonstrates the deterministic-first automation pattern, but the surfaces around it were "data-first, not insight-first" (lists everywhere, no synthesis, no memory). The first thickness layer just landed: the engagement-detail page now opens with a "Today" overview synthesising status counts, per-risk coverage, a heuristic-driven attention list, and a recent-activity timeline before the existing detail tables — same data, told as a story instead of scattered into rows. The matcher track is paused while the remaining thickness layers (living context panels, cross-reference graph, auto-synthesis on matcher output, active QA feedback) and the launch blockers (report export, sampling engine, client portal, OCR ingestion, sync, multi-provider LLM, licensing) are addressed.

The Module 6/7/8 ribbon (fieldwork → evidence → findings) still spans four ITGC/ITAC families with eleven wired rules. From an engagement detail page an auditor can add a library control (clones risks + tests into the engagement), upload encrypted `DataImport`s tagged by purpose, and run a rule-based matcher — `UAM-T-001` (terminated-but-active), `UAM-T-002` (periodic-recertification completeness and remediation), `UAM-T-003` (dormant accounts), `UAM-T-004` (orphan accounts), `CHG-T-001` (change-management approval-before-deployment), `CHG-T-002` (dev-vs-deploy segregation-of-duties), `BKP-T-001` (backup performance), `ITAC-T-001` (Benford's-Law first-digit analysis on a transaction population), `ITAC-T-002` (duplicate-transaction detection on a transaction population), `ITAC-T-003` (boundary / threshold analysis on a transaction population), or `ITAC-T-004` (recurring-amount detection across counterparties on a transaction population) — to produce a `TestResult`, elevate exceptions to a draft `Finding` with severity, drill into the test's Working Paper to see control context + run history + CCCER findings (condition / criteria / cause / effect / recommendation) + test-scoped evidence, and browse every raw upload, matcher report, and free-form attachment in the engagement-level Evidence table. All matchers flow through one generic `run_matcher` dispatcher with a `purpose_tag → DataImport.id` override map. All mutations are attributable (SyncRecord + ChangeLog + ActivityLog + EvidenceProvenance) and cross-firm isolated.

Earlier foundations remain: authentication + encrypted DB, Ed25519-verified library baseline (now v0.1.0 + v0.2.0 + v0.3.0 + v0.4.0 + v0.5.0 + v0.6.0 + v0.7.0), Clients / Engagements CRUD, and the Library browse route.

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

**Immediate next up** (ordered as the design-philosophy thickness queue, then carried-over backlog):

1. **Living context panels** — the largest gap between CLAUDE.md's intent and reality. Sidebars on every working-paper / control / finding screen surfacing "things commonly missed", "what good evidence looks like", "typical client pushback", last year's narrative on the same control if recurring, the open findings touching this control, and prior matcher run history. Static content shipped with the library plus dynamic recall from the engagement DB; rule-based, no LLM needed for v1.
2. **Cross-reference graph view** — visualise the risk ↔ control ↔ test ↔ evidence ↔ finding chain. Click a risk; see every control, test, recent evidence, open findings. Click a finding; trace it back to control and risk. Data model already supports this; the UI doesn't expose it.
3. **Storytelling Word/PDF report export** — first deliverable that leaves the app. Cross-references that link procedures to findings, embedded matcher visualisations, risk heatmap on the cover. Picks up the deferred report-export blocker.
4. **Smart upload classifier** — drop a CSV; the app reads the headers, says "this looks like an AD export — 487 enabled accounts, 23 disabled — suggest tagging as `ad_export` and running UAM-T-003". Reduces purpose-tag-picking to one click. Rule-based.
5. **Auto-synthesis on matcher output** — cluster a 47-exception matcher result into "32 from one user, 10 service accounts, 5 dormant >365 days" so the auditor reads three lines instead of forty-seven.
6. **Active QA feedback** — real-time completeness / workflow / calibration checks as the auditor works ("test result shows exception but no finding raised", "severity High is in the top quartile for similar findings — are you sure?").
7. **Hash-chain audit-trail export** — cryptographic proof of work performed, signed with a firm key. Builds on the existing ChangeLog + ActivityLog. Showcase / portfolio-flex piece.
8. **Engagement playback** — chronological replay of who did what when and what evidence supported each conclusion. Showcase / portfolio-flex piece.

**Carried over from earlier roadmap (still real, lower priority than the thickness layers above):**

- **More rule matchers** — UAR carries four rules, CHG two, BKP one, ITAC four. Next natural extensions: a privileged-access review for UAR (fifth UAR rule, reconciling AD "Domain Admins" or application "sysadmin" membership against an approver-list snapshot) and a round-number-prevalence rule for ITAC (fifth ITAC rule, complements recurring-amount detection by flagging populations with an unnaturally high share of round-number postings).
- **Matcher override picker UI** — the backend accepts an arbitrary `overrides: Record<purpose_tag, data_import_id>` map; the UI currently always sends `null`, so the command picks the newest matching import per tag. Picker appears when auditors start asking "run this against last quarter's export".
- **Review-note annotations on the working paper** — inline margin notes on a test that reviewers can tick to clear, preserving the audit trail.
- **Attach-evidence-to-finding UI** — `finding_attach_evidence` / `finding_detach_evidence` are wired and tested; surface them from the CCCER editor.
- **Schema cleanup** — drop `User.argon2_hash` and `User.master_key_wrapped` (future migration). Authoritative copies live in `identity.json`.
- **Password change UI** — the `change_password` command is already registered; Settings needs a form that calls it.
- **Zeroise master key in memory** — add `ZeroizeOnDrop` to the MK buffer before it reaches SQLCipher. Defensive; not a known leak.

**Library follow-ups** (do when they become load-bearing, not before):

- **Firm-override UI** — editing a library entry creates a `FirmOverride` row keyed by `(code, library_version)`. Browse is read-only today; overrides were deliberately out of scope for the baseline.
- **Library updates in-app** — today the baseline is compiled in. A later flow will let a firm drop a new `.json` + `.sig` pair into an inbox directory, verify, and install as a new version with `superseded_by` lineage. The loader already supports the upgrade path; only the pick-up-from-disk wiring is missing.
- **Broader framework coverage** — ISO 27001 and PCI DSS mappings remain deferred. v0.2.0 carries COBIT 2019 + NIST CSF forward; ISO/PCI land in a later bundle.

---

## Decision log

### 2026-04-25 — Engagement "Today" overview lands; pivot from rule catalogue to thickness layers

First feature shipped that's not a matcher. The engagement-detail page used to open as a stack of lists — Data imports, Tests, Test results, Findings, Evidence — each table inert and self-contained. The user's feedback after touching it was direct: "the app feels empty, like a glorified file manager". That diagnosis is right. Eleven matchers across four families is enough to demonstrate the deterministic-first automation pattern; what's missing is the *thickness* — synthesis, memory, and connection across the surfaces the auditor actually works in. Shipping more rules wouldn't fix the empty feeling; layering insight on top of every existing screen will.

The first thickness layer: a "Today" overview as the new hero block at the top of the engagement-detail page. Same data the app already had, told as a story. Status counts, per-risk coverage strip, heuristic-driven attention list, and a recent-activity timeline — all in one round-trip from a new `engagement_overview` Tauri command, all synthesised in Rust rather than aggregated client-side. Below it the existing detail tables stay put as the "drill-down" surface; auditors who want the lists can still find them.

**Backend** (`app/src-tauri/src/commands/engagements.rs`, +~590 lines):
- New Tauri command `engagement_overview(engagement_id)` returning a single `EngagementOverview` struct with five sub-shapes: `EngagementHeader` (name, client, status, period, library version, lead partner), `StatusCounts` (23 counters spanning controls, risks, tests-by-status, results-by-outcome, findings-by-status, findings-by-severity, evidence, data imports), `Vec<RiskCoverageEntry>` (per-risk roll-up with computed `coverage_state`), `Vec<AttentionItem>` (heuristic flags ordered high → medium → low), and `Vec<RecentActivityEntry>` (last 12 ActivityLog rows joined to User for actor name).
- `coverage_state` computed in Rust: `uncovered` if no controls linked, `untested` if controls but no test results, `tested_clean` if all results are pass, `tested_with_exceptions` if at least one exception or fail. The frontend reads this string for both colour coding and ordering.
- **Five attention-list heuristics** (deterministic, SQL-driven, ordered high → medium → low): tests in `in_review` (matcher exception awaiting decision); test results with `outcome='exception'` not linked to any finding; findings of severity Critical or High still in `draft` status; risks with no controls (computed by inverting the JSON-array `EngagementControl.related_engagement_risk_ids_json` column into a set); controls with no tests yet. Each item carries `kind` / `priority` / `label` / `entity_type` / `entity_id` so the frontend can deep-link into the relevant detail surface when those routes exist.
- **Why a single command, not five list-and-aggregate calls.** The frontend could in principle compute these from the existing `engagementListTests` / `…ListTestResults` / `…ListFindings` / `…ListEvidence` / `…ListDataImports` calls. It would also pull around 5× the row volume across the wire (rows, not counts) and re-aggregate the same numbers in JS on every keystroke. The single command sends one query plan to SQLite per concern — `COUNT(*) GROUP BY status` instead of `SELECT * → js.filter().length` — and returns a few hundred bytes instead of a few hundred KB on a real engagement.
- Risk-coverage roll-up is intentionally split across four queries (risks, controls + parsed JSON risk-id arrays, tests, results) plus the findings query, then assembled in Rust. SQLite has JSON1 functions but compiling the join inside one statement obscures the rule and makes the migration to a future `EngagementRiskControlLink` join table harder. Five queries / five small loops keeps the rule readable.
- 5 unit tests cover the new command: empty engagement returns zero counts and only the engagement-creation activity row; full scenario rolls up correctly across risks/controls/tests/results/findings (asserts counts, the per-risk `tested_with_exceptions` state, that all four expected attention-item kinds appear, and that `high` priority items precede `medium`); risk with controls but no tests is marked `untested` and surfaces a `control_no_test` low-priority item; clean population is marked `tested_clean`; cross-firm engagement id returns `NotFound` (authz check). All 5 pass; full-suite count went from 197 to 202.
- **Test-harness fix while here**: the existing `seeded_db` helper called `tmp_path("seeded")` with a literal-string suffix. With Cargo's parallel runner two seeded_db calls within one nanosecond produced identical paths and the second `install_baseline_bundles` collided on the LibraryRisk UNIQUE constraint. Suffixed the path with the firm id (`seeded-{firm_id}`) so parallel tests using different firms never collide. Pre-existing brittleness, surfaced when adding new tests using parallel firm ids.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`):
- Five new TS interfaces mirroring the backend (`EngagementHeader`, `StatusCounts`, `RiskCoverageEntry`, `AttentionItem`, `RecentActivityEntry`, `EngagementOverview`) plus the `engagementOverview(engagementId)` invoke wrapper.
- Inserted as the first content section of `EngagementDetail.svelte`, before the existing Data imports / Tests / Results / Findings / Evidence tables. Loaded in parallel with the existing five list calls (one extra await on the same `Promise.all`).
- Visual structure: header with engagement metadata (period, client, status, lead partner, library version) → 4-card status counter grid → risk-coverage strip with colour-coded left rail per coverage state → two-column row showing attention list (left) and recent activity (right). Cards / pills / faint-muted-accent classes follow the existing Dashboard.svelte design language; the only new tokens are coverage-state left-rail colours and three rating-pill variants.
- **Live counters via `refreshOverview()`** — calls hooked into the five mutations that change overview state: data import upload, library control add, matcher run, finding elevation, finding edit save, evidence upload. Narrow refresh (one query, ~few hundred bytes) rather than re-running the full page load. Failures are logged and the prior overview stays visible — an overview-refresh hiccup must not blow up the rest of the surface.
- Activity timeline uses a `relativeTime(secs)` helper: "Just now" / "n minutes ago" / "n hours ago" / "n days ago" up to seven days, then full-date fallback. Removes the cognitive load of reading raw timestamps and keeps the timeline readable at a glance.

**Verified**: `cargo test --lib` 202 passing (197 → 202: +5 overview tests, no regressions). `npm run check` 94 files 0 errors 0 warnings. `npm run build` 140.95 KB JS / gzip 43.30 KB; CSS 37.81 KB / gzip 5.70 KB (+8.5 KB JS over the ITAC-T-004 release — the new view's markup, CSS, and helper functions; +3.86 KB CSS).

**Three design calls worth keeping:**
1. **Single-command overview, not five list-and-aggregate calls.** The synthesis happens in Rust over indexed columns, returns a few hundred bytes, and means the frontend never has to know which counts are derived from which list. Future thickness layers (graph view, smart-classifier banner) can call the same command and add their own client-side rendering without re-implementing aggregation. Keeps the Today view's cost flat regardless of how many tables grow underneath it.
2. **Heuristic attention list, not LLM-suggested.** Deterministic rules for what surfaces in the "Needs attention" panel. Five heuristics today, ordered high → medium → low so the auditor sees what to act on first. New heuristics are one SQL query each. Hews to the CLAUDE.md "rules first, ML second, LLM last" hierarchy and means the panel is reproducible across runs — important for review and regulatory inspection.
3. **Today is additive, not a replacement.** The existing Data imports / Tests / Test results / Findings / Evidence tables stay below the Today block unchanged. Auditors who learned the list-driven flow keep working; new auditors hit the synthesis first. No workflow churn, no migration of existing UX.

**Deferred (explicitly not in scope for v1 of the Today view)**:
- **Recurrence callouts** — "Finding F-005 from 2025 is unaddressed; this is its second consecutive year on this client". Needs cross-engagement comparison and a recurring-finding detection layer on top. The schema already carries `prior_engagement_risk_id` / `prior_engagement_control_id` / `prior_test_id`; lighting them up in the overview is a separate workstream.
- **Trend sparklines** — exception count over time, evidence accumulation rate. The data is in `ActivityLog` and `TestResult.performed_at` but visual sparklines need a charting library decision the rest of the app hasn't faced yet.
- **Click-through navigation from attention items** — every `AttentionItem` carries `entity_type` + `entity_id` so the frontend can deep-link, but the route surfaces (e.g. opening the working paper for a flagged test, jumping to a specific finding's editor) need a router refactor to accept anchor parameters. Today the items render as informational; click-through arrives with the router work.
- **Live polling / websocket refresh** — the Today view is a snapshot. The five mutation handlers refresh it; nothing pulls automatically. Real-time refresh waits for the sync layer, which is its own workstream.

**Files of note**:
- `app/src-tauri/src/commands/engagements.rs` — new `engagement_overview` command with helper functions, six new exported structs, plus 5 unit tests and a parallel-safe path fix in the test harness
- `app/src-tauri/src/lib.rs` — registered `engagement_overview` in the Tauri invoke handler list
- `app/src/lib/api/tauri.ts` — six new TS interfaces and an `engagementOverview(engagementId)` invoke wrapper
- `app/src/lib/routes/EngagementDetail.svelte` — new Overview block as first section, `refreshOverview()` helper hooked into five mutation handlers, four new Today-specific helper functions (`coverageStateLabel`, `ratingPillClass`, `formatActivityAction`, `relativeTime`), ~150 lines of new CSS

---

### 2026-04-25 — Recurring-amount detection across counterparties matcher (ITAC-T-004) shipped

Fourth ITAC rule lands, completing the initial ITAC quartet. The rule scans a transaction population for monetary amounts that recur across many *distinct* counterparties — not the same posting twice (that's duplicates), not amounts clustering near a threshold (that's boundary), and not a digit-distribution anomaly (that's Benford), but the same dollar figure showing up at five or more unrelated parties. Genuine business activity rarely produces that signature; clusters with it warrant investigation as potential template-driven postings, kickback patterns, structured payments under internal thresholds, or fabricated records assembled from a single placeholder figure.

**Closes the ITAC ring.** All four rules now share one upload (`transaction_register` purpose tag) and one library control (`ITAC-C-001` analytical plausibility testing), so an auditor running them in sequence on the same export gets four independent angles on the population's plausibility: digit distribution (`ITAC-T-001`), exact-key duplication (`ITAC-T-002`), threshold-gaming clustering (`ITAC-T-003`), and counterparty-diversity recurrence (`ITAC-T-004`). The four rules complement rather than overlap — each catches a fingerprint the others miss.

**Library v0.7.0** (`app/src-tauri/resources/library/v0.7.0.json`):
- Full carry-forward of v0.6.0 (4 risks, 6 controls, 10 test procedures) plus one new test procedure: `ITAC-T-004` "Recurring-amount detection across counterparties" under the existing `ITAC-C-001` control. `sampling_default: "none"` (population-level test), `automation_hint: "rule-based"`. Six-step test narrative parallels the other three ITAC procedures' shape: obtain → confirm whole population + distinguish from legitimate uniform-fee schedules → run matcher → investigate flagged amounts (uniform fee, regulatory levy, recurring subscription, or escalation) → document → record exception. Evidence checklist adds an explicit item for the population's expected-pricing-model description so the auditor can distinguish "amount recurring because the firm has a flat fee" from "amount recurring with no plausible cause".
- Signed via `tools/sign-library-bundle/` against `~/.config/audit-app/signing/library.key`. Loader adds `BUNDLE_V0_7_0` / `BUNDLE_V0_7_0_SIG` as the seventh `install_bundle` call. `baseline_bundle_loads_into_fresh_db` extended to assert v0.7.0 shape (4 risks / 6 controls / 11 test procedures / 11 checklists / 12 framework mappings) and that v0.6.0's rows are now superseded.

**Pure matcher** (`app/src-tauri/src/matcher/itac_recurring.rs`, ~410 lines):
- `run_recurring_amounts(transactions: &Table) -> RecurringAmountReport`. One pass to populate a `BTreeMap<i64, GroupAccum>` keyed by absolute amount in integer cents. Each `GroupAccum` carries a `BTreeMap<String, String>` of normalised-counterparty-key → first-seen display form, so the distinct-counterparty count and the "first 10 alphabetical counterparties" listing both fall out of the same structure with deterministic ordering.
- `RecurringAmountReport { rule: "recurring_amounts", rows_considered, rows_skipped_unparseable, rows_skipped_zero, rows_skipped_no_counterparty, rows_skipped_below_significance, min_group_rows, min_distinct_counterparties, min_amount_cents, recurring_group_count, total_recurring_rows, exceptions: Vec<RecurringAmountException> }`. `RecurringAmountException { kind: "recurring_amount_group", display_amount, amount_cents, row_count, distinct_counterparty_count, counterparties (up to 10, alphabetical), row_ordinals (full), sample_rows (up to 5) }`. Distinct-counterparty count and counterparty list are deliberately separate — the count is unbounded (auditor reads the full diversity), the listing is capped to keep the report JSON bounded.
- **Three independent gates, all required to flag a group**: `row_count >= MIN_GROUP_ROWS` (5), `distinct_counterparty_count >= MIN_DISTINCT_COUNTERPARTIES` (5), `amount_cents >= MIN_AMOUNT_CENTS` (10_000 = $100). The distinct-counterparty gate is the *primary* signal — without it the rule is just "single vendor billing the same amount 5 times" which is duplicates territory, not recurring-amount territory. The row-count gate is technically redundant with the distinct-counterparty gate (5 distinct implies 5 rows) but stays explicit for symmetry and so a future tuning that lifts MIN_DISTINCT_COUNTERPARTIES without touching MIN_GROUP_ROWS still has both gates documented. The significance gate filters out the small-fee / verification-posting noise that recurs naturally across many vendors and would otherwise drown the report.
- **Shared parsing path**: imports `parse_amount` + `AMOUNT_CANDIDATES` from `itac_benford`, and `COUNTERPARTY_CANDIDATES` (newly promoted to `pub(super)`) from `itac_duplicates`. All four ITAC rules now go through one currency-parser implementation; counterparty header resolution is the same across duplicates and recurring. No drift on edge cases like `(1,234.56)` or `USD 100.00`.
- **Sign-insensitive via absolute value**: refunds and reversals fold into the same magnitude bucket. A pattern of "the same $1,234.56 going to many vendors" is the signal whether posted as positive (payment out) or negative (refund in); both shapes are equally suspicious. Same convention as Benford / duplicates / boundary.
- **Verbatim counterparty match for the diversity count**: normalised (trimmed + lower-cased) counterparty strings. `Acme Ltd.` and `acme ltd.` count as one party; `Acme Ltd.` and `Acme Limited` count as two. Fuzzy vendor identity stays out of scope — same reasoning as duplicates: an alias table is firm-specific configuration, not a data-derivable assumption. A unit test pins this — five rows that look like five vendors but normalise to four distinct keys correctly fail the gate.
- **Exception order**: descending by `amount_cents` so the auditor sees the largest dollar fish first. Stable on ties via secondary `row_count` desc, though in practice `amount_cents` alone is unique per group (it *is* the group key).
- **Counterparty list capped at 10, full count exposed**: a 50-counterparty cluster fires the rule as intended but should not balloon the report JSON. Auditor reads the full count, the first 10 names (alphabetical for determinism), and uses `row_ordinals` to pull the rest from the source CSV when investigating. Sample-row cap of 5 follows the same pattern as the duplicates rule.
- 18 unit tests covering: flags 5-distinct cluster passes both gates; 10-row single-counterparty fails distinct gate; 4-distinct fails distinct gate; below-significance amount skipped at row time even with 10 distinct vendors; header variants normalise (`Transaction Amount` / `Counter Party`); rows missing counterparty counted skipped not grouped; unparseable amounts skipped; zero amounts skipped; counterparty match case + whitespace insensitive; negative amounts fold to abs for grouping; currency symbols + thousands separators normalise; exception order descending by amount; sample rows capped at 5 with full row_ordinals retained; counterparty list capped at 10 with full distinct_counterparty_count retained; constants round-trip into the report; missing amount column skips every row as unparseable; missing counterparty column skips every row as no_counterparty; single counterparty repeating in one group does not double-count distinct (5 rows / 5 distinct, not 5 rows / 6 distinct). All 18 pass.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::ItacRecurringAmounts` added; `for_test_code` routes `ITAC-T-004 → ItacRecurringAmounts`; dispatch branch calls `run_itac_recurring_amounts` (same five-arg shape as the other three single-input ITAC helpers — no `now` because the rule has no time-component).
- `run_itac_recurring_amounts` mirrors the boundary / duplicates helper shape: resolve population via `transaction_register` / `transactions` / `gl_export` / `primary` aliases (same list every ITAC rule uses), `load_csv_table`, call the pure matcher, build a two-variant summary that surfaces the three tuning constants in the message body so the auditor reads "gates: >= 5 rows AND >= 5 distinct counterparties AND >= 10000 cents" alongside the count, JSON detail carrying all 12 reportable fields, `RuleOutcome::base(..., "IT application controls", "itac-recurring", ..., "Transaction register")`, `supporting_import = None` (population-level, single input).
- Five new `Option<...>` fields on `RuleOutcome` and `MatcherRunResult` (and `tauri.ts` mirror): `transactions_skipped_no_counterparty`, `transactions_skipped_below_significance`, `recurring_group_count`, `total_recurring_rows`, `recurring_min_amount_cents`. Re-uses the existing `transactions_considered` / `transactions_skipped_unparseable` / `transactions_skipped_zero` counters from the ITAC family block — those semantics are identical across all four rules. Slotted under a "Recurring-amount detection (ITAC-T-004)" comment so future readers see the rule grouping.
- Two integration tests covering: (1) `run_matcher_itac_recurring_flags_amount_across_many_counterparties` — 6 rows of $1,234.56 across 6 distinct vendors plus one $50 below-significance row → `outcome="exception"`, `exception_count=1`, `recurring_group_count=Some(1)`, `total_recurring_rows=Some(6)`, `transactions_skipped_below_significance=Some(1)`, `recurring_min_amount_cents=Some(10_000)`, other-rule counters (`digit_rows_evaluated`, `duplicate_group_count`, `thresholds_flagged`) remain `None`, `supporting_import_id` is `None`, `ActivityLog` shows one `matcher_run`, test status moves to `in_review`; (2) `run_matcher_itac_recurring_passes_on_diverse_population` — 8 rows each at a unique amount → `outcome="pass"`, `recurring_group_count=Some(0)`, `total_recurring_rows=Some(0)`. Both pass.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`):
- Five new `number | null` fields on `MatcherRunResult` in `tauri.ts` under a "Recurring-amount detection (ITAC-T-004)" comment block, mirroring the backend additions.
- `ITAC-T-004` added to `MATCHER_ENABLED_CODES` in both engagement-detail and working-paper routes.
- **No new PurposeTag entry needed.** The recurring rule consumes the same `transaction_register` tag the other three ITAC rules already use. `PURPOSE_OPTIONS` unchanged.

**Verified**: `cargo test --lib` 197 passing (177 before this rule + 18 new `itac_recurring` unit tests + 2 new integration tests = 197; library-loader assertions updated in place, no baseline regressions). `npm run check` 94 files 0 errors 0 warnings. `npm run build` 132.41 KB / gzip 40.87 KB (+20 bytes over the UAM-T-002 release — five new interface fields and a MATCHER_ENABLED_CODES entry on two routes, no new runtime code).

**Four design calls worth keeping:**
1. **Distinct-counterparty diversity is the primary gate, not row count.** The signal this rule chases is "same amount across *different* parties" — without diversity-of-counterparty the cluster is just a single vendor billing the same amount repeatedly, which the duplicates rule already catches. Setting MIN_DISTINCT_COUNTERPARTIES at 5 means the rule never fires on what duplicates would already flag, and the two rules surface genuinely different fingerprints.
2. **Significance floor at $100, not configurable yet.** Small repeating amounts ($5 fees, $1 verification postings, $0.50 micropayments) recur naturally across many vendors and would drown the report in noise that no auditor would investigate. $100 is round, defensible across small-to-mid African firm currencies, and excludes that noise without dropping any genuinely interesting cluster — fraud / structuring patterns at the relevant magnitudes are well above $100. A firm-configurable floor lands when an auditor asks; the field is exposed in `recurring_min_amount_cents` and the matcher's signature already takes `i64`-derived configuration, so wiring is mechanical.
3. **Verbatim counterparty match (case + whitespace insensitive only).** `Acme Ltd.` and `Acme Limited` stay as two distinct counterparties. Fuzzy vendor identity (`Acme Ltd.` ↔ `Acme Limited` ↔ `ACME LIMITED CO`) is a separate, heavier problem that needs a firm-provided alias table — exactly the same call duplicates already made. Treating them identically across both rules keeps the auditor's mental model consistent and means the same alias-table feature lights both up if and when it's built.
4. **Counterparty list capped at 10 alphabetical names; distinct count uncapped.** A 50-counterparty cluster firing the rule as intended shouldn't balloon the report JSON. The auditor reads the full count from `distinct_counterparty_count`, the first 10 names alphabetical (deterministic via the BTreeMap), and uses `row_ordinals` (kept in full) to pull the remaining 40 from the source CSV at investigation time. Same pattern as the duplicates rule's sample_rows + row_ordinals split.

**Deferred (explicitly not in scope)**:
- **Round-number-prevalence detection** — "this population has 40% round-thousand amounts where 5% would be expected" — related fraud signal but a fundamentally different shape (population-level statistic, not group-level cluster). Likely lands as `ITAC-T-005` once prioritised; would reuse the same upload but compute a single chi-square-style test against an expected round-vs-non-round distribution rather than grouping by amount.
- **Same-amount-different-period clustering** — "this $1,234.56 paid to many vendors *every quarter*" needs cross-period evidence comparison and belongs to the recurring-finding infrastructure, not this matcher.
- **Fuzzy counterparty matching** — `Acme Ltd.` ↔ `Acme Limited` resolution stays out of scope here for the same reason it's deferred in duplicates: needs a firm-configurable alias table. Both rules light up together when it lands.
- **Per-counterparty distinct-amount diversity** — "this vendor invoices at exactly the same amount every time" is a different shape (single-counterparty fingerprint) and would need a distinct rule. Belongs to vendor-master review, not transaction-population review.

**Files of note**:
- `app/src-tauri/src/matcher/itac_recurring.rs` — new matcher module
- `app/src-tauri/src/matcher/mod.rs` — registered `pub mod itac_recurring;`
- `app/src-tauri/src/matcher/itac_duplicates.rs` — `COUNTERPARTY_CANDIDATES` promoted to `pub(super)` for cross-rule reuse
- `app/src-tauri/resources/library/v0.7.0.json` (+ `.sig`) — new library bundle
- `app/src-tauri/src/library/loader.rs` — seventh bundle installed; baseline test extended
- `app/src-tauri/src/commands/testing.rs` — `MatcherRule::ItacRecurringAmounts`, dispatcher, `run_itac_recurring_amounts`, five new `RuleOutcome` + `MatcherRunResult` fields, two integration tests
- `app/src/lib/api/tauri.ts` — five new `MatcherRunResult` fields
- `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte` — `ITAC-T-004` in `MATCHER_ENABLED_CODES`

---

### 2026-04-24 — Periodic-recertification completeness and remediation matcher (UAM-T-002) shipped

Fourth UAR rule lands, closing the last un-wired library TP in every shipped bundle. The library has carried `UAM-T-002` since v0.1.0, but dispatch rejected it with `unsupported_test_code` because there was no matcher behind it. From today the rule is fully wired: an auditor uploads the periodic access review log alongside the usual AD / Entra export, runs the matcher, and gets back two distinct kinds of exceptions in one report — users in the AD population who are *missing* from the review log (completeness failure) and users in the review log whose review *raised an exception* but whose remediation is still open past a configurable ageing window (remediation failure). The matcher reports these separately so the auditor can see whether the control failed because the review didn't cover everyone or because exceptions raised by the review are rotting in a backlog.

**Completes the UAR quartet.** UAM-T-001 asks "are terminated employees still active in the system?" (backward: HR → AD). UAM-T-003 asks "are there accounts nobody has logged into for 90+ days?" (dormancy). UAM-T-004 asks "are there AD accounts with no owner in HR?" (orphans). UAM-T-002 asks "was the periodic review performed, did it cover the full population, and were the exceptions it raised actually closed?" — a control-operation question rather than a data-reconciliation one. Same AD export flows through all four (primary input, consistent across the family); the review log is the new supporting input.

**No new library bundle.** The test procedure was already authored and shipped — this release just wires the matcher behind the existing slot. `library_version = "v0.6.0"` assertions remain as-is.

**Pure matcher** (`app/src-tauri/src/matcher/access_review.rs`, added ~480 lines after the dormant-accounts rule):
- `run_periodic_recertification(review_log: &Table, ad: &Table, as_of_secs: i64, remediation_window_days: u32) -> RecertificationReport`. Reuses `parse_last_logon` + `normalise` + `is_enabled` helpers already in the module — adding a fourth rule to the existing file rather than spinning a new module keeps the UAR family's shared helpers in one place.
- `RecertificationReport { rule: "periodic_recertification", ad_rows_considered, ad_rows_skipped_disabled, ad_rows_skipped_unmatchable, review_rows_considered, review_rows_skipped_unmatchable, unreviewed_count, unremediated_count, remediation_check_applied, remediation_window_days, as_of_secs, exceptions: Vec<RecertificationException> }`. `RecertificationException { kind: "unreviewed_account" | "unremediated_exception", email, logon, ad_ordinal, review_ordinal, days_since_review, ad_row, review_row }` — the `kind` field carries the distinction so the frontend can group / filter by exception type within one report.
- **Two-leg design**: (1) completeness leg runs unconditionally — builds a `HashSet<String>` of normalised identifiers from the review log, iterates enabled AD rows, emits `unreviewed_account` for every enabled row whose email (or logon fallback) is absent from the set; (2) remediation leg is *opt-in by data* — only runs if the review log has both an exception-signal column (`exception_raised` / `exceptions_raised` / `exception` / `flagged` / `issue`) AND a remediation-status column (`remediation_status` / `status` / `resolution_status` / `close_status` / `closure_status`). When both are present, rows where the exception signal is true AND remediation is not `"closed"`/`"remediated"`/`"resolved"`/`"completed"` AND `review_date` is older than the configurable window emit `unremediated_exception`. When either column is missing, `remediation_check_applied = false` flows to the command layer and surfaces in the summary copy.
- **Four candidate lists** for header resolution: `REVIEW_DATE_CANDIDATES` (12 variants: `review_date`, `date_reviewed`, `certification_date`, `signoff_date`, etc.), `EXCEPTION_RAISED_CANDIDATES` (5 variants), `DECISION_CANDIDATES` (6 variants: `decision`, `review_decision`, `outcome`, `status`, `action`, `determination`), `REMEDIATION_STATUS_CANDIDATES` (5 variants). Review logs come in every shape imaginable — some firms have a dedicated boolean column, some fold it into the decision column, some only record the reviewed principal and the decision. The rule adapts to what's present.
- **Explicit-column-wins priority**: when both `exception_raised` and `decision` columns exist but disagree (e.g. `exception_raised=true` but `decision="approved"`), the explicit boolean wins. Avoids the ambiguity of inferring an exception from a decision column that might carry values like "remediate" or "revoke" that also look like action words. Covered by a dedicated unit test.
- **Conservative age gate**: unparseable / absent `review_date` flags conservatively. An unparseable date is itself a red flag for control operation (why is the review log missing when the review happened?), and silently passing a no-date row would defeat the point of the remediation-ageing check. If the firm is running reviews without dates, that should surface as a finding, not get absorbed into the "everything's fine" bucket.
- **Logon fallback**: when the review log lacks an email column but carries logon / SAM account name, the matcher falls back to logon-based matching. Both columns are compared after `normalise` (trim + lowercase). If neither the email nor the logon aligns between review log and AD, the AD row is counted as `ad_rows_skipped_unmatchable` and the review row as `review_rows_skipped_unmatchable` — visible separately in the report so the auditor can investigate inputs that refused to reconcile.
- **Age gate default**: `REMEDIATION_WINDOW_DAYS_DEFAULT = 90`. Ninety days is the industry norm for "stale open exception" — long enough that a legitimate remediation programme has had time to close the finding, short enough that a backlog accumulating past it signals control failure. The command takes a `u32` so a firm-override UI can later push it per engagement without changing the matcher.
- 15 unit tests covering: completeness flag on missing user, disabled AD skip, all-reviewed-clean pass, unremediated flag inside/outside 90-day window, absent / missing / unparseable `review_date`, empty `remediation_status`, missing exception-signal column (remediation leg skipped), missing remediation-status column (remediation leg skipped), explicit `exception_raised=true` beats disagreeing `decision="approved"`, logon fallback when email missing, unmatchable review rows counted separately, mixed `unreviewed_account` + `unremediated_exception` in one report. All 15 pass.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::UarPeriodicRecertification` added; `for_test_code` routes `UAM-T-002 → UarPeriodicRecertification`; dispatch branch passes `now` alongside tx / paths / engagement_id / master_key / overrides (rule needs `as_of_secs` for the age gate).
- `run_uar_periodic_recertification` helper (~100 lines): resolves `ad_export` / `entra_export` primary (same aliases as the other three UAR rules), then `access_review` / `access_review_log` / `recertification_log` supporting, `load_csv_table` each, calls the pure matcher with `REMEDIATION_WINDOW_DAYS_DEFAULT`, builds a four-variant summary from the `(exception_count, remediation_check_applied)` matrix — "N exception(s): X unreviewed / Y unremediated across Z users (remediation ageing window: 90 days)" vs the "check not applicable — review log lacks signal column" and "pass" variants — and emits JSON detail carrying all eleven report counters plus `remediation_check_applied` and `as_of_secs`. `population_ref_label = "AD export"` holds the UAR family convention; `supporting_evidence_ref_label = "Access review log"`.
- Six new `Option<…>` fields on `RuleOutcome` and `MatcherRunResult` (and `tauri.ts` mirror): `review_rows_considered`, `review_rows_skipped_unmatchable`, `unreviewed_count`, `unremediated_count`, `remediation_check_applied` (`Option<bool>`), `remediation_window_days`. Slotted under a "Periodic recertification (UAM-T-002)" comment so future readers see the rule grouping. Reuses existing `ad_rows_considered` / `ad_rows_skipped_disabled` / `ad_rows_skipped_unmatchable` counters from the UAR family block — those semantics are identical across all four rules.
- **Re-purposed dispatcher-rejection test**: `run_matcher_rejects_unsupported_test_code` previously used `UAM-T-002` as the probe since it was the only un-wired library TP. With this release every library TP is wired, so the test now clones a control, reads the resulting `UAM-T-002` test id, and `UPDATE Test SET code = 'XYZ-T-999'` to simulate an un-wired code — preserves the guard-rail for future library bundles that add TPs ahead of matchers without weakening as the matcher catalogue grows.
- Three integration tests covering: (1) `run_matcher_recertification_flags_unreviewed_and_unremediated` — 3-enabled + 1-disabled AD population with review log containing alice approved+closed + bob exception+open dated 2020-01-01 → `outcome="exception"`, `exception_count=2`, `unreviewed_count=1`, `unremediated_count=1`, `remediation_check_applied=true`, `remediation_window_days=90`, `supporting_import_filename="access_review.csv"`, `ActivityLog` shows one `matcher_run`; (2) `run_matcher_recertification_passes_when_all_users_reviewed_and_clean` — 2 enabled AD users both reviewed approved+closed → `outcome="pass"`, all counts zero; (3) `run_matcher_recertification_skips_remediation_when_review_log_lacks_columns` — review log with only an email column → `exception_count=1` (bob unreviewed), `unremediated_count=0`, `remediation_check_applied=false`, detail JSON carries `"remediation_check_applied":false`. All three pass.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`):
- Six new fields on `MatcherRunResult` in `tauri.ts` under a "Periodic recertification (UAM-T-002)" comment block, mirroring the backend additions (`review_rows_considered: number | null`, `review_rows_skipped_unmatchable: number | null`, `unreviewed_count: number | null`, `unremediated_count: number | null`, `remediation_check_applied: boolean | null`, `remediation_window_days: number | null`).
- `UAM-T-002` added to `MATCHER_ENABLED_CODES` in both engagement-detail and working-paper routes — the "Run matcher" button now appears for the periodic-recertification test.
- **New `access_review` PurposeTag**: added to both the `PurposeTag` TS union and the `PURPOSE_OPTIONS` array in `EngagementDetail.svelte` with the hint "Periodic recertification sign-off: one row per reviewed user, optionally with decision, review_date, and remediation_status columns". This is the first new PurposeTag added since the ITAC family landed — the three other UAR rules reuse `ad_export` and `hr_roster`, but the review log is a genuinely distinct artefact with its own expected shape.

**Verified**: `cargo test --lib` 177 passing (159 before this rule + 15 new `access_review` recertification unit tests + 3 new integration tests = 177; no baseline regressions). `npm run check` 94 files 0 errors 0 warnings. `npm run build` 132.39 KB / gzip 40.86 KB (+40 bytes over the ITAC-T-003 release — six new interface fields, one new PurposeTag entry with hint, and a MATCHER_ENABLED_CODES entry on two routes).

**Four design calls worth keeping:**
1. **AD primary, review log supporting — preserves UAR family consistency.** Every UAR rule takes the AD export as its primary input and reconciles a secondary source against it. Keeping UAM-T-002 on the same shape means `population_ref_label = "AD export"` across the whole family and the auditor's mental model doesn't reshuffle from one rule to the next. The rule's *subject* is the review log (the test name is "User access review completeness and remediation"), but its *primary population* is still AD — the completeness question only has meaning relative to who was supposed to be reviewed, which is the AD population.
2. **Opt-in-by-data remediation check, not a hard requirement.** The completeness leg runs on any review log that has at least an identifier column. The remediation leg only runs when the log also has both an exception-signal column and a remediation-status column. A firm whose review log records only `(email, reviewed_by, review_date)` without any closure tracking still gets the completeness report, `remediation_check_applied = false` in the summary, and no false negatives from the absence of columns the firm doesn't maintain. This keeps v1 deterministic — the matcher doesn't try to infer remediation status from ambient context — and lets the summary copy honestly say "check not applicable" rather than fabricate a pass.
3. **Conservative age gate over silent pass.** Rows with unparseable or absent `review_date` flag conservatively rather than slip through the ageing check. A missing review date is itself an audit issue (why wasn't the review dated?), and silently passing such rows would mean the rule can be defeated by simply not filling the date column. Being noisy about what you can't evaluate is safer than being silent.
4. **Explicit exception column beats decision column when both exist.** `exception_raised=true` with `decision="approved"` resolves to "exception was raised" — the explicit signal is more reliable than the inferred one. Decision columns carry remediation-ish verbs ("remove", "revoke", "retain") that are ambiguous as exception signals; the boolean is unambiguous. Covered by a unit test so any future refactor that changes priority breaks CI.

**Deferred (explicitly not in scope)**:
- **Firm-configurable ageing window** — 90 days is the default. A per-engagement override UI lands when an auditor asks; the command signature already takes `u32`, so wiring is a few lines when it's needed.
- **Cross-period completeness** — "this user was missed in Q1 *and* Q2" requires multiple review logs. Out of scope for a single-upload matcher; lands as a separate cross-period rule once the evidence layer supports comparing two imports of the same purpose tag.
- **Re-review follow-through** — "exceptions raised last quarter that were deferred with 'review again next quarter' should show up in this quarter's log" — richer workflow, belongs to the recurring-finding infrastructure on the roadmap, not this matcher.
- **Inferred exception from decision text** — if the firm's decision column carries free text like "remove access urgently", a richer NLP classifier could infer an exception. Out of scope: LLM classification is explicitly the last-resort tier per the automation hierarchy. Auditors whose logs only carry a decision column can rename the signal column to `exception_raised` and add it explicitly.

**Files of note**:
- `app/src-tauri/src/matcher/access_review.rs` — added `run_periodic_recertification` + 15 unit tests (total module now ~1300 lines carrying all four UAR rules)
- `app/src-tauri/src/commands/testing.rs` — `MatcherRule::UarPeriodicRecertification`, dispatcher branch, `run_uar_periodic_recertification` helper, six new `RuleOutcome` / `MatcherRunResult` fields, three new integration tests, re-purposed dispatcher-rejection test
- `app/src/lib/api/tauri.ts` — six new `MatcherRunResult` fields
- `app/src/lib/routes/EngagementDetail.svelte` — new `access_review` PurposeTag union entry + `PURPOSE_OPTIONS` entry with hint, `UAM-T-002` in `MATCHER_ENABLED_CODES`
- `app/src/lib/routes/WorkingPaper.svelte` — `UAM-T-002` in `MATCHER_ENABLED_CODES`

---

### 2026-04-24 — Boundary / threshold analysis matcher (ITAC-T-003) shipped

Third ITAC rule lands: a fraud / control-gaming detector that scans a transaction population for clusters of amounts sitting just *below* a known approval threshold. A payment at $9,950 walks through the approval that a payment at $10,050 does not, so transactions deliberately split, timed, or priced to stay under an authorisation rule bunch up in the window below the threshold. The rule counts rows in `[T - W, T)` versus `[T, T + W]` for each threshold `T` (with `W = 5% × T`), and flags thresholds whose below window holds at least 10 rows AND whose below/above ratio is ≥ 2.0. A threshold whose above window is empty is treated as unbounded ratio and flagged on the absolute count alone — the "entire cluster is below, nothing above" shape is the signal we most want to catch.

**Completes the initial ITAC triad.** `ITAC-T-001` asks "does the first-digit distribution look natural?"; `ITAC-T-002` asks "are there exact repeats the population shouldn't contain?"; `ITAC-T-003` asks "is anyone gaming a dollar threshold?" All three share the same input (`transaction_register` purpose tag), the same library control (`ITAC-C-001` — analytical plausibility testing), and the same "no sampling, whole population" philosophy. Auditor runs all three off one upload and gets three independent angles on the same integrity question. No new PurposeTag, no new UI surface — matcher layer only.

**Library v0.6.0** (`app/src-tauri/resources/library/v0.6.0.json`):
- Full carry-forward of v0.5.0 (4 risks, 6 controls, 9 test procedures) plus one new test procedure: `ITAC-T-003` "Boundary / threshold analysis on transaction population" under the existing `ITAC-C-001` control. `sampling_default: "none"` (population-level test), `automation_hint: "rule-based"`. Six-step test narrative parallels the Benford and duplicates procedures' flow (obtain → confirm completeness → run matcher → investigate each flagged threshold → escalate per policy → record exception) with one extra evidence-checklist item: the organisation's documented approval matrix, so the auditor can distinguish "flagged threshold maps to a real authorisation boundary we should investigate" from "flagged threshold is coincidental because the firm never approves at that value".
- Signed via `tools/sign-library-bundle/` against `~/.config/audit-app/signing/library.key`. Loader adds `BUNDLE_V0_6_0` / `BUNDLE_V0_6_0_SIG` as the sixth `install_bundle` call. `baseline_bundle_loads_into_fresh_db` extended to assert v0.6.0 shape (4 risks / 6 controls / 10 test procedures / 10 checklists / 12 framework mappings) and that v0.5.0's rows are now superseded.

**Pure matcher** (`app/src-tauri/src/matcher/itac_boundary.rs`, ~560 lines):
- `run_boundary_thresholds(transactions: &Table) -> BoundaryReport`. Single pass to build `Vec<(abs_amount, &Row)>` from the population; for each threshold `T` in `BOUNDARY_THRESHOLDS`, count rows falling in `[T - W, T)` (below window) and `[T, T + W]` (above window); flag when the gates are met and emit a sample of up to 5 contributing rows (below-window preferred).
- `BoundaryReport { rule: "boundary_threshold", rows_considered, rows_skipped_unparseable, rows_skipped_zero, thresholds_evaluated, thresholds_flagged, window_fraction, min_below_count, flag_ratio, exceptions: Vec<BoundaryException> }`. `BoundaryException { kind: "boundary_threshold_cluster", threshold, below_window_low, below_window_high, above_window_low, above_window_high, below_count, above_count, ratio: Option<f64>, sample_rows }`. `ratio` is `None` when `above_count == 0`; the frontend renders that as "∞ / no above-window rows" rather than divide by zero.
- **Fixed threshold list**: 1k, 5k, 10k, 25k, 50k, 100k, 250k, 500k, 1M (all in the population's currency). Chosen to match the round-number authorisation bands auditors see in practice across small-to-mid African firms. Firms that want to override or extend get a firm-configurable list later once a UI exists; over-specifying is cheap because thresholds whose entire below window sits above the population's max amount are silently dropped. The common case is covered by the defaults.
- **Five tunable constants** exposed in the report so the auditor can read them without opening source: `BELOW_WINDOW_FRACTION = 0.05` (5% window), `MIN_BELOW_COUNT = 10` (absolute-count gate to guard against small-sample noise — 3-below-vs-1-above is a 3× ratio but not a signal), `FLAG_RATIO = 2.0` (proportional gate: natural distributions put roughly equal counts in two equal-sized adjacent windows, so 2× excess is the sweet spot), `SAMPLE_ROWS_PER_THRESHOLD = 5`.
- **Shared currency parser**: uses `parse_amount` + `AMOUNT_CANDIDATES` via `use super::itac_benford::{...}` — same path the duplicates rule already takes. All three ITAC matchers stay on one currency-parsing implementation; the `(1,234.56)` and `USD 100.00` and `100.00 CR` edge cases are handled identically across Benford, duplicates, and boundary. Also uses `super::csv::{find_column, Row, Table}` for header resolution.
- **Drop condition**: thresholds are dropped from evaluation when the entire below window sits above the population's max amount (`below_low > max_amount`). *Not* when the threshold itself exceeds max — a population with max 9,900 should still evaluate the 10k threshold because the below window `[9_500, 10_000)` reaches back into the population. Earlier attempt used the simpler `threshold > max_amount` and failed six tests: the fix is on the window's lower edge, not the threshold's value.
- **Sign-insensitive via absolute value**: a refund of -$9,950 contributes the same signal as a payment of +$9,950. The gaming pattern is independent of posting direction — if someone is splitting a $19,900 payment into two $9,950 chunks, both chunks matter; if someone reversing a $10,000 posting drops a -$9,950 near the threshold, that's equally suspicious. Matches the Nigrini convention already used by the Benford rule.
- 14 unit tests: flags cluster just below 10k with correct ratio / below-count / sample shape; balanced 10-vs-10 distribution passes despite meeting absolute gate; 5-vs-0 cluster fails absolute gate (too small); 12-vs-0 passes unbounded-ratio path and flags; thresholds above population max skipped; multiple thresholds flagged in the same population; exception order ascending by threshold (stable diffs); zero and unparseable amounts skipped not grouped; negative amounts fold to absolute; missing amount column skips every row; currency-symbol + comma normalisation feeds the window comparison correctly; sample rows capped + prefer below-window; sample fills from above-window when below is exhausted; constants round-trip into report for auditor visibility. All 14 pass.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::ItacBoundaryThreshold` added; `for_test_code` routes `ITAC-T-003 → ItacBoundaryThreshold`; dispatch branch calls `run_itac_boundary_thresholds`.
- `run_itac_boundary_thresholds` mirrors the Benford / duplicates helpers' single-input shape: resolve the population via `transaction_register` / `transactions` / `gl_export` / `primary` purpose-tag aliases (same list as the other two ITAC rules — all three share the upload), `load_csv_table`, call the pure matcher, build a two-variant summary ("N threshold(s) flagged across X considered row(s) (Y threshold(s) evaluated)" / "No boundary thresholds flagged across X considered row(s) (Y threshold(s) evaluated)"), JSON detail carrying population counters plus the three tuning constants, `RuleOutcome::base(..., "IT application controls", "itac-boundary", ..., "Transaction register")`, `supporting_import = None` (population-level, single input).
- Two new `Option<i64>` fields on `RuleOutcome` and on `MatcherRunResult` (and `tauri.ts` mirror): `thresholds_evaluated`, `thresholds_flagged`. Re-uses the existing `transactions_considered` / `transactions_skipped_unparseable` / `transactions_skipped_zero` counters from the ITAC family block — same semantics the Benford and duplicates rules already use. Slotted after the duplicates fields with a comment noting the rule they belong to.
- Two integration tests: `run_matcher_itac_boundary_flags_cluster_below_threshold` (15-row population with 12 at 9,900 + 3 at 10,100 → `outcome="exception"`, `exception_count=1`, `thresholds_flagged=Some(1)`, Benford / duplicates counters remain `None`, `ActivityLog` shows one `matcher_run`) and `run_matcher_itac_boundary_passes_on_balanced_population` (20-row population split evenly 10/10 → `outcome="pass"`, `exception_count=0`, `thresholds_flagged=Some(0)`). Both pass.

**Frontend** (`app/src-tauri/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`):
- Two new `number | null` fields on `MatcherRunResult` in `tauri.ts` under a "Boundary / threshold analysis (ITAC-T-003)" comment block, mirroring the backend additions.
- `ITAC-T-003` added to `MATCHER_ENABLED_CODES` in both engagement-detail and working-paper routes — the "Run matcher" button now appears for the boundary-threshold test just as it does for Benford and duplicates.
- **No new PurposeTag entry needed.** The boundary rule consumes the same `transaction_register` tag the other two ITAC rules already use. `PURPOSE_OPTIONS` unchanged.

**Verified**: `cargo test --lib` 159 passing (143 before this rule + 14 new `itac_boundary` unit tests + 2 new integration tests = 159; library-loader assertions updated in place, no baseline regressions). `npm run check` 94 files 0 errors 0 warnings. `npm run build` 132.18 KB / gzip 40.78 KB (+30 bytes over duplicates-only — two new interface fields and an enable-set entry, no new runtime code). Chrome DevTools / e2e deferred until the picker UI arrives — today's work is pure backend + interface contract.

**Three design calls worth keeping:**
1. **Fixed threshold list, not firm-configurable.** The v1 list (1k / 5k / 10k / 25k / 50k / 100k / 250k / 500k / 1M) covers the round-number authorisation bands seen in practice across small-to-mid African firms. Making this configurable adds UI that isn't paying for itself yet — over-specifying is cheap because thresholds above the population's max are silently dropped, and firms whose approval matrix doesn't follow round numbers can dismiss flagged thresholds at investigation time. The firm-configurable version lands when an auditor asks for it.
2. **Fraction-of-threshold window, not a fixed dollar width.** A 5% window scales naturally: the 1k threshold checks ±50, the 1M threshold checks ±50,000. A fixed $500 window would be absurdly wide for 1k and absurdly narrow for 1M. The 5% figure is round, defensible, and narrow enough that a balanced natural distribution produces small counts on both sides of every threshold — wide enough that a real clustering pattern still registers.
3. **Below window exclusive, above window inclusive.** `[T - W, T)` vs `[T, T + W]`. A row at exactly the threshold belongs in the "above" window (it triggers the authorisation), not the "below" window (which is the dodge-the-rule zone). This is semantically the signal the rule is looking for: someone splitting at exactly $10,000 isn't the gaming pattern; someone splitting at $9,950 to stay under is.

**Deferred (explicitly not in scope)**:
- **Firm-configurable threshold list** — v1 ships fixed; configurable threshold UI arrives when an auditor asks. The constants are re-exported so a future config layer can override them without changing the matcher.
- **Time-window segmentation** — "just-below-threshold at month-end" is a richer procedure that requires date parsing and period boundaries. Separate, later rule.
- **Inference of the firm's actual approval matrix** — that's an organisational input (policy doc, approval-workflow export), not a data-derived one. Checking the generic band and letting the auditor dismiss thresholds that don't apply is the right tradeoff for a default procedure.
- **Round-number / same-amount clustering** — "every amount in this population is a round thousand" or "the same amount shows up at 50 different counterparties" — related fraud signal, separate test with its own tuning parameters. Likely lands as `ITAC-T-004` once prioritised.

**Files of note**:
- `app/src-tauri/src/matcher/itac_boundary.rs` — new matcher module
- `app/src-tauri/src/matcher/mod.rs` — registered `pub mod itac_boundary;`
- `app/src-tauri/resources/library/v0.6.0.json` (+ `.sig`) — new library bundle
- `app/src-tauri/src/library/loader.rs` — sixth bundle installed; test updated
- `app/src-tauri/src/commands/testing.rs` — `MatcherRule::ItacBoundaryThreshold`, dispatcher, `run_itac_boundary_thresholds`, two new `RuleOutcome` + `MatcherRunResult` fields, two integration tests
- `app/src/lib/api/tauri.ts` — two new `MatcherRunResult` fields
- `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte` — `ITAC-T-003` in `MATCHER_ENABLED_CODES`

---

### 2026-04-24 — Duplicate-transaction detection matcher (ITAC-T-002) shipped

Second ITAC rule lands: an exact-triple duplicate detector that groups a transaction population by `(amount, counterparty, date)` and flags every group containing two or more rows. Designed for accounts-payable and accounts-receivable duplication review — genuine business activity rarely produces identical postings on the same day to the same counterparty for the same amount, so any such cluster is an investigation lead (double-posted invoice, duplicated manual journal, fabricated record with a copy-pasted row). Not a fuzzy-match engine: two rows with slightly different vendor spellings or adjacent dates stay unflagged, by design. Fuzzy duplication is a separate, heavier test and needs either a firm-provided alias table or a vendor-normalisation layer — both out of scope for the first pass.

**Complements Benford, doesn't overlap it.** `ITAC-T-001` asks "does the population's leading-digit distribution look natural?" from first principles. `ITAC-T-002` asks "are there exact repeats the population shouldn't contain?" from pairwise equality. The two share the same input (`transaction_register` purpose tag) and the same library control (`ITAC-C-001`, analytical plausibility testing), so the auditor can run both in sequence on the same upload and they surface different symptoms of the same concern. Reusing the purpose tag also means no new PurposeTag enum entry is needed on the frontend.

**Library v0.5.0** (`app/src-tauri/resources/library/v0.5.0.json`):
- Full carry-forward of v0.4.0 (4 risks, 6 controls, 8 test procedures) plus one new test procedure: `ITAC-T-002` "Duplicate-transaction detection on transaction population" under the existing `ITAC-C-001` control. `sampling_default: "none"` (population-level test), `automation_hint: "rule-based"`. Six-step test narrative mirrors the Benford procedure's flow (obtain → confirm completeness → run matcher → investigate each flagged group → escalate per policy → record exception).
- Signed via `tools/sign-library-bundle/` against `~/.config/audit-app/signing/library.key`. Loader adds `BUNDLE_V0_5_0` / `BUNDLE_V0_5_0_SIG` as the fifth `install_bundle` call. `baseline_bundle_loads_into_fresh_db` extended to assert v0.5.0 shape (4 risks / 6 controls / 9 test procedures / 9 checklists / 12 framework mappings) and that v0.4.0's rows are now superseded.

**Pure matcher** (`app/src-tauri/src/matcher/itac_duplicates.rs`, ~430 lines):
- `run_duplicate_transactions(transactions: &Table) -> DuplicateReport`. Iterates rows, parses amount + counterparty + date, builds a grouping key, and accumulates rows into a `BTreeMap<(amount_cents, counterparty_key, date), GroupAccum>` keyed by `(i64, String, String)` — BTreeMap is deliberate so exception iteration order is stable across runs, same pattern as the SoD rule's `intersection_keys.sort()`.
- `DuplicateReport { rule: "duplicate_transactions", rows_considered, rows_skipped_unparseable, rows_skipped_zero, rows_skipped_no_key, duplicate_group_count, total_duplicate_rows, exceptions: Vec<DuplicateException> }`. `DuplicateException { kind: "duplicate_transaction_group", display_amount, amount_cents, counterparty, display_counterparty, date, row_count, row_ordinals, sample_rows }`. Sample rows capped at `SAMPLE_ROWS_PER_GROUP = 5` to keep the report a reasonable size on pathological populations. `amount_cents` is the integer grouping key; `display_amount` is the first-seen raw string — the auditor sees the shape the client exported.
- `COUNTERPARTY_CANDIDATES` (24 headers: `counterparty`, `vendor`, `vendorname`, `supplier`, `customer`, `payee`, `account`, `party`, `name`, `entity`, `beneficiary`, `merchant`, `description`, etc.) and `DATE_CANDIDATES` (20 headers: `date`, `transaction_date`, `posting_date`, `posted_on`, `invoice_date`, `document_date`, `gl_date`, etc.). Kept broad because ERP exports, payments-platform reports, and accounting-package dumps all spell the columns differently.
- **Shared currency parser**: exposed `parse_amount` and `AMOUNT_CANDIDATES` as `pub(super)` in `matcher/itac_benford.rs`. `itac_duplicates` imports them so the same currency-symbol / ISO-code / parentheses-negative / thousands-separator handling works identically for both rules. Avoids ~50 lines of duplicated parsing logic and prevents the two rules from drifting apart on edge cases like `(1,234.56)` or `USD 100.00`.
- **Integer-cents key**: amounts are converted via `(amount.abs() * 100.0).round() as i64` before use in the grouping tuple. Float keys would alias via `NaN` and precision issues; `-100.00` and `100.00` collapse to the same magnitude bucket (sign is considered deliberately out of scope for "two postings of the same amount" — a reversal paired with an original posting is itself a duplicate worth investigating).
- **Normalisation**: counterparty matched case-insensitively and whitespace-collapsed. Date strings compared *as the source provided them* — mixing `2024-05-01` with `05/01/2024` does not merge the two into one group. This is restrictive by design: parsing dates into a canonical form would require the auditor to resolve regional ambiguity (US vs EU) and mask genuine data-quality issues; comparing verbatim means the matcher reports exactly what's in the export.
- 15 unit tests: flags exact pair, flags group of three, passes on fully-unique population, same amount + vendor but different dates do *not* merge, header-variant normalisation, amount with `$` + commas normalises for match, different vendors at same amount + date are not duplicates, zero amounts skipped, missing counterparty or missing date counted as `rows_skipped_no_key`, unparseable amounts counted but never grouped, counterparty match case-insensitive, mixed date formats stay separate, absolute-value sign handling, sample rows cap at 5, deterministic exception ordering. All 15 pass.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::ItacDuplicateTransactions` added; `for_test_code` routes `ITAC-T-002 → ItacDuplicateTransactions`; dispatch branch calls `run_itac_duplicate_transactions`.
- `run_itac_duplicate_transactions` mirrors the Benford helper's single-input shape: resolve the population via `transaction_register` / `transactions` / `gl_export` / `primary` purpose-tag aliases (same list as Benford — the two rules share the upload), `load_csv_table`, call the pure matcher, build a two-variant summary ("N duplicate groups covering X rows across Y considered rows" / "No duplicate transactions found across Y considered rows"), JSON detail carrying all 7 counters, `RuleOutcome::base(..., "IT application controls", "itac-duplicates", ..., "Transaction register")`, `supporting_import = None` (population-level, single input), plus the three new counters.
- Three new `Option<i64>` fields on `RuleOutcome` and on `MatcherRunResult` (and `tauri.ts` mirror): `transactions_skipped_no_key`, `duplicate_group_count`, `total_duplicate_rows`. Re-uses the existing `transactions_considered` / `transactions_skipped_unparseable` / `transactions_skipped_zero` counters from the ITAC family block — those semantics are identical across the two rules (a parseable non-zero monetary amount on a transaction row). Slotted directly after the Benford fields with a comment noting the rule they belong to so future readers don't mistakenly reuse `duplicate_group_count` for another rule's similarly-shaped output.
- Two integration tests: `run_matcher_itac_duplicates_flags_exact_repeats` (4-row population with one duplicate pair → `outcome="exception"`, `exception_count=1`, `duplicate_group_count=1`, `total_duplicate_rows=2`, Benford-only counters remain `None`, `ActivityLog` shows one `matcher_run`) and `run_matcher_itac_duplicates_passes_on_unique_population` (3 rows with same vendor + amount but different dates → `outcome="pass"`, `exception_count=0`, `duplicate_group_count=Some(0)`). Both pass.

**Frontend** (`app/src-tauri/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`):
- Three new `number | null` fields on `MatcherRunResult` in `tauri.ts` under a "Duplicate-transaction detection (ITAC-T-002)" comment block, mirroring the backend additions.
- `ITAC-T-002` added to `MATCHER_ENABLED_CODES` in both engagement-detail and working-paper routes — the "Run matcher" button now appears for the duplicate-detection test just as it does for Benford.
- **No new PurposeTag entry needed.** The duplicate rule consumes the same `transaction_register` tag the Benford rule already registered. `PURPOSE_OPTIONS` unchanged.

**Verified**: `cargo test --lib` 143 passing (128 before this rule + 15 new `itac_duplicates` unit tests + 2 new integration tests = 143; library-loader assertions updated in place, no baseline regressions). `npm run check` 94 files 0 errors 0 warnings. `npm run build` 132.15 KB / gzip 40.78 KB (+30 bytes over Benford-only — three new interface fields and an enable-set entry, no new runtime code). Chrome DevTools / e2e deferred until the picker UI arrives — today's work is pure backend + interface contract.

**Three design calls worth keeping:**
1. **Exact-form duplicate detection, not fuzzy.** Two rows must match on all three of normalised amount, normalised counterparty, and verbatim date string. Fuzzy vendor matching, near-date bucketing, and amount-tolerance windows are separate, heavier tests that need configuration the baseline can't assume (vendor alias tables, tolerance thresholds, locale-specific date parsers). The exact form is still a genuinely useful audit signal on its own — double-postings and copy-paste fabrication almost always produce exact triples — and it's cheap to run.
2. **Absolute value for amount match.** `(-100.00, Acme, 2024-05-01)` matches `(100.00, Acme, 2024-05-01)` and is flagged as a pair. An original posting + its reversal *is* a duplicate in the sense the rule is looking for: if both exist on the same day to the same counterparty, the reversal should have cancelled the original, and the fact that both survived is the signal. Firms that want signed matching can filter afterwards — easier than trying to unflag here.
3. **Date verbatim, not parsed.** The matcher compares `"2024-05-01"` to `"2024-05-01"` directly and will not merge `"2024-05-01"` with `"05/01/2024"`. Parsing dates into a canonical form is a minefield across US/EU formats, Excel serials, timezone-stripped timestamps, and locale-specific month names — the safest default is "trust the client's export". If the same date appears in two different forms within one population, that's itself a data-quality issue worth flagging separately, not masking here.

**Deferred (explicitly not in scope)**:
- **Fuzzy counterparty / near-date duplicate detection** — needs firm-configurable thresholds or an alias table. Would land as `ITAC-T-00X` if ever prioritised.
- **Cross-period duplicate detection** — the rule only sees one upload at a time. Year-over-year "this vendor received the same payment on the same day last year" needs the recurring-finding infrastructure that's already on the roadmap, not this matcher.
- **Amount-tolerance windows** (e.g. "flag groups within 1% of each other") — same issue: needs configuration and moves the test from "exact repeats" to "suspicious clusters". Different test code entirely.
- **Counterparty aliasing / normalisation beyond trim+lower** — "Acme Ltd." vs "ACME LIMITED" stay separate until the firm provides an alias table. The matcher deliberately doesn't pretend to resolve identity.

**Files of note**:
- `app/src-tauri/src/matcher/itac_duplicates.rs` — new matcher module
- `app/src-tauri/src/matcher/mod.rs` — registered `pub mod itac_duplicates;`
- `app/src-tauri/src/matcher/itac_benford.rs` — `parse_amount` + `AMOUNT_CANDIDATES` promoted to `pub(super)` for sibling-module reuse
- `app/src-tauri/resources/library/v0.5.0.json` (+ `.sig`) — new library bundle
- `app/src-tauri/src/library/loader.rs` — fifth bundle installed; test updated
- `app/src-tauri/src/commands/testing.rs` — `MatcherRule::ItacDuplicateTransactions`, dispatcher, `run_itac_duplicate_transactions`, three new `RuleOutcome` + `MatcherRunResult` fields, two integration tests
- `app/src/lib/api/tauri.ts` — three new `MatcherRunResult` fields
- `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte` — `ITAC-T-002` in `MATCHER_ENABLED_CODES`

---

### 2026-04-24 — Dev-vs-deploy segregation-of-duties matcher (CHG-T-002) shipped

Second CHG rule lands: a structural segregation-of-duties check that reconciles the production-deployment tool's permission list against the source repository's write-access list. Any user appearing in both holds "deploy-to-production" and "author-a-change" capabilities simultaneously and could push their own code without review. The matcher flags the intersection; the auditor inspects each user and confirms either (a) they have since been removed from one side, or (b) a documented compensating control (four-eyes review, post-deployment monitoring) operated during the period. It does not try to identify compensating controls itself — that's evidence-based, not data-driven.

Distinct from CHG-T-001's existing per-change `approver_is_implementer` exception: that asks "did anyone bypass review on *this* deployed change?" from the change-log record itself. CHG-T-002 asks the wider structural question "who *could* bypass review across the whole period?" from the permission matrices. Overlap is a feature — the two surface different symptoms of the same control gap.

**No library bundle change.** `CHG-T-002` and its parent control `CHG-C-002` already shipped in v0.4.0 alongside the Benford rule. The library entry was already signed, loaded, and covered by the `baseline_bundle_loads_into_fresh_db` test; today's change is purely the matcher + command-layer wiring + integration tests + frontend enable. Smallest possible incremental backend change to activate an already-frozen library spec.

**Pure matcher** (`app/src-tauri/src/matcher/change_management.rs`):
- `run_sod_dev_vs_deploy(deploy_access: &Table, source_access: &Table) -> SoDReport`. Builds two `HashMap<username_key, (ordinal, raw_row)>` maps, intersects the keys, emits one `SoDException` per user in the intersection with the first-occurrence ordinal from each side.
- `SoDReport { rule, deploy_rows_considered, deploy_rows_skipped_unmatchable, source_rows_considered, source_rows_skipped_unmatchable, deploy_unique_users, source_unique_users, intersecting_users, exceptions: Vec<SoDException> }`. `SoDException { kind: "user_has_dev_and_deploy", username, deploy_row, source_row, deploy_ordinal, source_ordinal }`. Username on the exception is the *normalised* key (trimmed, lower-cased); the two raw rows preserve the auditor's original CSV columns for reviewing the context.
- `SOD_USERNAME_CANDIDATES` covers 21 plausible column headers — `user`, `username`, `login`, `samaccountname`, `principal`, `email`, `upn`, `member`, `developer`, etc. Kept deliberately broad since deploy tools, CI/CD platforms, source hosts, and IAM exports all spell the identity column differently. Match on canonicalised headers (lower-cased, `_` / `-` / space stripped), same as every other matcher in the module.
- Normalisation is trim + lower-case only. Cross-form identity mapping (display name vs email vs SAM account) is deliberately out of scope — that needs a firm-provided alias table, not heuristics. Intersection keys are sorted before emission so exception ordering is stable across runs.
- Dedup within one side: a user who appears on multiple deploy rows (two release-manager groups, say) counts once in `deploy_unique_users` and produces at most one exception. First occurrence's ordinal and raw row are kept.
- Seven unit tests in the same file: happy path (1 intersection across disjoint rest), disjoint pass path, case + whitespace-insensitive matching, rows with missing username counted as skipped, within-side dedup, deterministic exception ordering, header-variant normalisation.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::ChgSodDevVsDeploy` added; `for_test_code` routes `CHG-T-002 → ChgSodDevVsDeploy`; dispatch branch calls `run_chg_sod_dev_vs_deploy`.
- `run_chg_sod_dev_vs_deploy` mirrors the two-input reconciliation shape used by `run_uar_terminated_but_active` and `run_uar_orphan_accounts`: resolve the deploy export via `deploy_permissions` / `deployment_access` / `primary` purpose-tag aliases, resolve the source export via `source_access` / `source_repo_access` / `supporting` aliases, `load_csv_table` both, call the pure matcher, build a two-variant summary ("N users with both deploy-to-production and source-write access" / "No SoD overlap across X deploy-permission rows and Y source-access rows"), JSON detail with all counters + both unique-user counts, `RuleOutcome::base(..., "Change management", "change-management-sod", ..., "Deployment permission export")`, `supporting_import = Some(source_import)`, plus the five new counters.
- Five new `Option<i64>` fields on `RuleOutcome` and on `MatcherRunResult` (and `tauri.ts` mirror): `deploy_rows_considered`, `deploy_rows_skipped_unmatchable`, `source_rows_considered`, `source_rows_skipped_unmatchable`, `intersecting_users`. Slotted under a new "CHG SoD (dev-vs-deploy)" sub-block inside the CHG family block, labelled with a comment noting "two permission-list inputs, not a change log" so future readers don't try to reuse the change-log counters for this rule.
- `report_kind_slug = "change-management-sod"` — distinct from CHG-T-001's `"change-management"` slug so the persisted report blobs can be told apart. Report filenames become `change-management-sod-CHG-T-002.json` versus `change-management-CHG-T-001.json`.
- Two integration tests: `run_matcher_chg_sod_flags_users_on_both_deploy_and_source` (three deploy users × three source users × one overlap → one exception; asserts `rule: "sod_dev_vs_deploy"`, `outcome: "exception"`, all five new counters populated, `supporting_import_filename == "source.csv"`, Test flips to `in_review`, `ActivityLog.matcher_run` fires once, detail JSON contains both `"sod_dev_vs_deploy"` and `"intersecting_users"`, CHG approval-family counters stay `None`); `run_matcher_chg_sod_passes_on_disjoint_permission_lists` (disjoint lists → zero exceptions, `outcome: "pass"`, `intersecting_users: Some(0)`).

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/{EngagementDetail,WorkingPaper}.svelte`):
- Five new `number | null` fields on `MatcherRunResult`, slotted under a new "CHG SoD (dev-vs-deploy)" comment inside the CHG family block.
- `MATCHER_ENABLED_CODES` gets `"CHG-T-002"` in both routes.
- EngagementDetail's `PurposeTag` gets `"deploy_permissions"` and `"source_access"`; `PURPOSE_OPTIONS` grows two matching rows ("Deploy permissions — Production deployment tool's role or permission matrix" / "Source repository access — Source host or change-authoring tool's access export"). Auditors tag the files at upload time; the override picker UI is still deferred (`overrides: null` from the run button).
- No new Working Paper counter surfaced — existing UI shows `rule`, `outcome`, and `exception_count`; per-user intersection lives in the JSON detail blob and the downloadable report.

**Verified**:
- `cargo test --lib` — 126 passing (up from 117 after Benford). Seven new unit tests in `matcher::change_management::tests::sod_*`, two new integration tests in `commands::testing::tests::run_matcher_chg_sod_*`.
- `npm run check` — 94 files, 0 errors, 0 warnings.
- `npm run build` — clean vite bundle (~132.12 KB, gzip ~40.77 KB).

**Design call — the rule stops at "who's in both sets"**: identifying *which* overlaps have an acceptable compensating control is evidence-based, not data-driven, and deliberately outside the matcher's scope. The exception list is the *auditor-facing* follow-up queue; each user gets individually reviewed, and the disposition goes in the working paper's CCCER, not the matcher report.

**Design call — exact-form normalisation only**: usernames match on trim + lower-case. If the deploy tool exports "Alice Example" and the source host exports "alice@example.com", the matcher won't bridge that — firms need an identity alias table for cross-form matching, which is a separate feature. Keeping the rule strict limits false positives at the cost of requiring auditors to upload exports with comparable identifier columns.

**Design call — distinct from CHG-T-001's `approver_is_implementer`**: the existing CHG-T-001 rule already flags same-person-approved-and-deployed on individual change records. CHG-T-002 asks the structural question across the period. Both rules can fire; overlap is expected and informative.

**Deliberately deferred**:
- Cross-form identity matching (email ↔ SAM account ↔ display name) via a firm-provided alias table.
- Compensating-control registry — today the auditor documents compensating controls in the working-paper CCCER free text. A structured registry (per user, with effective dates) is a later schema addition.
- Override picker UI. Still `null`.
- Privileged-access-review (the "who holds Domain Admins?" question) is shaped similarly to this rule — permission snapshot × approver list — but deliberately tracked as the next UAR rule rather than bolted onto the CHG family.

**Files of note**: `app/src-tauri/src/matcher/change_management.rs` (+~180 lines for the SoD rule + tests), `app/src-tauri/src/commands/testing.rs` (new dispatch branch + helper + 5 new counters + 2 integration tests), `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`.

### 2026-04-24 — Benford's-Law first-digit matcher (ITAC-T-001) shipped, ITAC family opens

First IT application controls rule lands, opening a new ITGC/ITAC family alongside UAR / CHG / BKP. The matcher takes an exported transaction population and tests whether its leading-digit distribution follows Benford's Law — the empirical regularity that in naturally-occurring numeric data, digit `d` appears as the leading non-zero digit with frequency `log10(1 + 1/d)`. Finance, expense, and revenue populations that are fabricated, rounded to clean thresholds, or selectively omitted tend to deviate visibly. A clean population clears the test in seconds; a flagged one points the auditor at which digit is over- or under-represented before they start asking targeted questions.

Deliberately shipped as the ITAC opener because it's the most general-purpose test in the family (any numeric population with a reasonable magnitude spread qualifies: GL entries, expense claims, revenue line items, inventory receipts) and it's entirely deterministic — no ML, no LLM, no sampling. Fits the rules-first automation tier cleanly.

**Pure matcher** (`app/src-tauri/src/matcher/itac_benford.rs`, new module):
- Exports `BenfordReport { rule, rows_considered, rows_skipped_unparseable, rows_skipped_zero, digit_rows_evaluated, digit_counts: [u64; 9], digit_observed_frequencies: [f64; 9], digit_expected_frequencies: [f64; 9], chi_square: Option<f64>, chi_square_critical: f64, digit_deviation_threshold: f64, min_digit_rows: usize, exceptions: Vec<BenfordException> }` and `BenfordException { kind, digit, observed_frequency, expected_frequency, deviation, note }`. Three exception discriminators: `population_too_small` (digit count below the chi-square threshold), `digit_frequency_anomaly` (per-digit observed-vs-expected gap), `chi_square_exceeds_critical` (global goodness-of-fit failure when no single digit breached the per-digit threshold).
- Constants: `EXPECTED_FREQUENCIES: [f64; 9]` (Benford's law P(d) = log10(1 + 1/d) for d ∈ 1..9), `CHI_SQUARE_CRITICAL_DF8_ALPHA05 = 15.507` (hardcoded critical value at 8 degrees of freedom, α = 0.05 — avoids pulling in a p-value library), `DIGIT_DEVIATION_THRESHOLD = 0.02` (2 percentage points), `MIN_DIGIT_ROWS = 300` (population-size floor below which chi-square is not meaningful).
- `parse_amount` handles the messy input shapes Simba expects from real African-market exports: currency symbols (`$`, `£`, `€`, `¥`, `₦`, `R`, `Z$`), ISO codes (`USD`, `EUR`, `ZAR`, `ZWL`, `NGN`, `KES`, ...), thousands separators (`,`, space, `'`), parenthesised-negative accounting convention (`(150) = -150`), and `CR` / `DR` trailing suffixes. `leading_digit` takes the absolute value and scales it into `[1, 10)` by repeated ×10 / ÷10 until a truncated integer falls in `1..=9` — handles `0.000732` (→ 7) and `8.4e6` (→ 8) identically.
- `run_benford_first_digit(&Table)`:
  - Iterates the amount column (auto-detected from the usual suspects: `amount`, `value`, `net`, `total`, ...), counts `rows_skipped_unparseable` and `rows_skipped_zero` (Benford does not apply to zeros).
  - If `digit_rows_evaluated < 300` → single `population_too_small` exception, `chi_square: None`, no digit-level work.
  - Otherwise computes chi-square, emits one `digit_frequency_anomaly` per digit with `|observed − expected| > 0.02`.
  - If chi-square > 15.507 but no per-digit rule fired, falls through to a single `chi_square_exceeds_critical` exception — the distribution is globally off even though no single digit stands out.
- Ten unit tests: pure-Benford population passes, uniform distribution fails (each digit contributes), population-too-small shortcircuits before chi-square, currency-symbol parsing, paren-negative parsing, CR/DR suffix parsing, leading-digit scaling (both small and large magnitudes), empty-amount-column handling, zero-only column handling, and the `chi_square_exceeds_critical` fallthrough shape.

**Library v0.4.0** (`app/src-tauri/resources/library/v0.4.0.json` + `.sig`):
- Full carry-forward: v0.3.0's 3 risks / 5 controls / 7 test procedures re-emitted, plus `ITAC-R-001` ("Transaction records are fabricated, manipulated, or selectively omitted during entry or reporting"), `ITAC-C-001` ("Transaction populations are subjected to analytical plausibility testing", COBIT 2019 DSS06.01 / NIST CSF DE.AE-2), and `ITAC-T-001` ("Benford's Law first-digit analysis on transaction population", `sampling_default: "none"`, `automation_hint: "rule-based"`, four-item evidence checklist: transaction register export, amount column definition, Benford report output, follow-up documentation for any flagged digits). Published-at epoch 1777032000 (2026-04-24 12:00:00 UTC), sitting twelve hours after v0.3.0's 1776988800.
- Same carry-forward reasoning as v0.3.0 — `control_code_to_id` is populated intra-bundle only, `current_library_version` uses `MAX(library_version) WHERE superseded_by IS NULL`, so a thin "ITAC-only" bundle would leave prior UAR/CHG/BKP rows un-superseded and broken. The convention is now established: every library bundle re-emits every prior entity.
- Signed via the `tools/sign-library-bundle/` CLI against `~/.config/audit-app/signing/library.key`. Baked into `library/loader.rs` as `BUNDLE_V0_4_0` + `BUNDLE_V0_4_0_SIG` via `include_bytes!` / `include_str!`, installed as the fourth `install_bundle` call in `install_baseline_bundles`. The `baseline_bundle_loads_into_fresh_db` test grew to assert the v0.4.0 shape: 4 risks current, 6 controls, 8 test procedures, 8 checklists, 12 framework mappings; v0.3.0 rows all have `superseded_by` pointing at their v0.4.0 equivalent; `ITAC-T-001` sits under `ITAC-C-001` at `library_version = "v0.4.0"`.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::ItacBenfordFirstDigit` added; `for_test_code` routes `ITAC-T-001 → ItacBenfordFirstDigit`; dispatch branch calls `run_itac_benford_first_digit(...)`.
- `run_itac_benford_first_digit` resolves the transaction-register import via `transaction_register` / `transactions` / `gl_export` / `primary` purpose-tag aliases (no supporting import — Benford is a single-population test), `load_csv_table`s it, calls the pure matcher, builds a three-variant summary based on whether exceptions contain `population_too_small` / are empty / carry digit anomalies, writes JSON detail with all counters + `chi_square` + `chi_square_critical` + `min_digit_rows` + `digit_deviation_threshold`, and constructs `RuleOutcome::base(..., "IT application controls", "itac-benford", ..., "Transaction register")`. Sets the four new ITAC counters on the outcome: `transactions_considered` (total rows in the register), `transactions_skipped_unparseable` (rows whose amount column didn't yield a number), `transactions_skipped_zero` (rows with amount = 0), `digit_rows_evaluated` (the subset that actually contributed to the digit distribution). `supporting_import` is `None`.
- `RuleOutcome` grows four `Option<i64>` fields for the ITAC family. `MatcherRunResult` and `app/src/lib/api/tauri.ts`'s TypeScript mirror gain the same four, slotted into a new "IT application controls family" block in the type. Kept as separate fields from the UAR/CHG/BKP counters — each family carries different auditor-facing semantics and conflating them would lose meaning.
- Three integration tests. `run_matcher_itac_benford_flags_uniform_digit_distribution`: 900 rows with amounts `[1, 2, 3, ..., 9, 1, 2, ...]` — a deliberately un-Benford distribution — asserts the test flips to `in_review`, `exception_count > 0`, at least one `digit_frequency_anomaly`, and the chi-square is present and exceeds the critical value. `run_matcher_itac_benford_passes_on_benford_like_population`: 900 rows drawn from powers of 1.12 (a classic Benford-compliant generator) — asserts no exceptions, `outcome == "passed"`, and chi-square is present and below the critical value. `run_matcher_itac_benford_flags_small_population`: 150 rows — asserts exactly one `population_too_small` exception, `chi_square: null` in the detail JSON, `digit_rows_evaluated < 300`.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/{EngagementDetail,WorkingPaper}.svelte`):
- `MatcherRunResult` grows the four ITAC counters under a new "IT application controls family" comment block.
- `EngagementDetail.svelte`: `PurposeTag` gains `"transaction_register"`; `PURPOSE_OPTIONS` gains "Transaction register — Export of the in-scope transaction population, with an amount column" so auditors can tag the file correctly at upload time. `MATCHER_ENABLED_CODES` gains `"ITAC-T-001"` so the Run button appears on that test row.
- `WorkingPaper.svelte`: `MATCHER_ENABLED_CODES` gains `"ITAC-T-001"` so the Run button appears on the test's working-paper detail page.
- No new Working Paper counter surfaces — the existing UI shows `rule`, `outcome`, and `exception_count`; the per-digit numbers live in the JSON detail blob and the downloadable report.

**Verified**:
- `cargo test --lib` — 117 passing (up from 104). New: ten matcher unit tests in `matcher::itac_benford::tests` and three integration tests in `commands::testing::tests` (happy, sad, too-small). `baseline_bundle_loads_into_fresh_db` updated to expect the v0.4.0 shape.
- `npm run check` — 94 files, 0 errors, 0 warnings.
- `npm run build` — clean vite bundle (~131.82 KB, gzip ~40.67 KB).

**Design call — chi-square without a p-value function**: hardcoded the critical value (15.507 at 8 df, α = 0.05) rather than pulling in a statistics crate. The rule is deliberately conservative — if the observed distribution clears that bar, the test passes; if not, exceptions fire. A firm that wants a stricter α can add a separate rule later; the per-digit threshold (2 percentage points) gives more actionable information than a p-value anyway.

**Design call — 300-row minimum**: below ~300 rows, chi-square loses power and any per-digit deviation is just noise. Chose to emit a single `population_too_small` exception rather than compute and display misleading digit frequencies. The auditor can see the threshold in the detail JSON and decide whether to re-export with a wider period.

**Design call — supporting import = None for ITAC**: Benford is a one-sided test on a single population. Unlike UAR (AD + HR reconcile) or CHG (change register alone but bolts onto an approval log in the second rule), ITAC-T-001 needs exactly one file. The RuleOutcome still carries the `supporting_import` slot — future ITAC rules that want an expected-totals reconciliation can populate it.

**Deliberately deferred**:
- Second-digit Benford analysis (more sensitive for rounding fraud, noisier to interpret). First-digit is the default in the audit literature.
- Excel/XLSX native parsing — today the transaction register must be CSV. A `xlsx` parser + `.xls` legacy reader come with the broader messy-HR-data ingester work.
- Per-engagement threshold overrides — auditors currently can't dial the 2pp deviation threshold or the 300-row floor from the UI. These are firm-level defaults for now.
- Override picker UI. Still `null`.

**Files of note**: `app/src-tauri/src/matcher/itac_benford.rs` (new, ~450 lines), `app/src-tauri/src/matcher/mod.rs`, `app/src-tauri/resources/library/v0.4.0.json`, `app/src-tauri/resources/library/v0.4.0.json.sig`, `app/src-tauri/src/library/loader.rs`, `app/src-tauri/src/commands/testing.rs`, `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`.

### 2026-04-24 — Orphan-accounts UAR matcher (UAM-T-004) shipped

Third UAR rule lands: AD accounts that don't appear anywhere in the HR master list. The matcher inverts the terminated-but-active reconciliation — instead of asking "is a known leaver still enabled in AD?", it asks "is this enabled AD account on the authoritative employee roster?". An unmatched enabled account is an orphan: a service account someone never registered, a contractor whose engagement ended without HR closing the loop, a long-stale break-glass login. The HR master is the *current* employee list; terminated people belong on the leavers list consumed by `run_terminated_but_active`, not here.

**Pure matcher** (`app/src-tauri/src/matcher/access_review.rs`):
- `OrphanReport { rule, ad_rows_considered, ad_rows_skipped_disabled, ad_rows_skipped_unmatchable, hr_rows_considered, exceptions: Vec<OrphanException> }` and `OrphanException { kind: "orphan_account", email, logon, ad_ordinal, ad_row }`. `hr_rows_considered` is new to the UAR family — the size of the authoritative master so the auditor can sanity-check that the HR file covers who they expect.
- `run_orphan_accounts(ad: &Table, hr_master: &Table) -> OrphanReport`. Builds a pair of `HashSet<String>` over HR emails and HR logons (normalised, empty strings discarded), then iterates AD rows: skip disabled into `ad_rows_skipped_disabled`, skip rows with neither email nor logon into `ad_rows_skipped_unmatchable`, flag rows whose email *and* logon are both absent from the master. Matching uses the same email-primary / logon-fallback logic as the terminated rule — a logon-only hit is enough to *clear* an account (errs toward fewer exceptions: a known employee with a renamed mailbox shouldn't be an orphan). `ad_rows_considered` is the full AD row count (parallel to the other two UAR reports), so disabled rows still show up in the total; the `*_skipped_*` counters are breakdowns the auditor reads off the summary card.
- Six unit tests: exact-flag happy path, disabled-row-skipping, logon-only-fallback when HR lacks an email column, both-identifiers-missing skip path, empty-exceptions pass path, and case/whitespace-insensitive matching.

**Library v0.3.0** (`app/src-tauri/resources/library/v0.3.0.json` + `.sig`):
- Full carry-forward baseline: three risks, five controls, seven test procedures (six from v0.2.0 + new `UAM-T-004`), seven checklists, ten mappings. `superseded_by` fields left null on v0.3.0 rows; the loader's `mark_prior_versions_superseded` chains v0.2.0 → v0.3.0 automatically.
- `UAM-T-004` sits under `UAM-C-002` alongside `UAM-T-002` (periodic recertification) and `UAM-T-003` (dormant accounts). Name "Review of orphan application accounts", six procedure steps (obtain AD + HR master, reconcile, investigate unmatched, classify, confirm disablement, document). `sampling_default: "none"`, `automation_hint: "rule-based"`, four-item evidence checklist (AD export with enabled flag, HR master roster, service-account register, disablement evidence / management justification for kept accounts).
- Carry-forward chosen over a thin "UAM-T-004 only" bundle: (a) the loader's `control_code_to_id` is populated per-bundle only, so a test procedure referencing `UAM-C-002` forces re-emitting that control; (b) that control references `UAM-R-001` via `related_risk_codes`, forcing risks to carry too; (c) `current_library_version` uses `MAX(library_version) WHERE superseded_by IS NULL` on LibraryControl — a thin bundle would leave v0.2.0 controls un-superseded and break the query. Easier to make this the baseline pattern for every future library bundle.
- Signed via the existing `tools/sign-library-bundle/` CLI against the private key at `~/.config/audit-app/signing/library.key`. Baked into `app/src-tauri/src/library/loader.rs` as `BUNDLE_V0_3_0` + `BUNDLE_V0_3_0_SIG` via `include_bytes!` / `include_str!`, installed as the third `install_bundle` call in `install_baseline_bundles`.

**Command layer** (`app/src-tauri/src/commands/testing.rs`):
- `MatcherRule::UarOrphanAccounts` added, `for_test_code` routes `UAM-T-004 → UarOrphanAccounts`, dispatch branch calls `run_uar_orphan_accounts(...)`.
- `run_uar_orphan_accounts` mirrors the terminated-but-active shape: resolve AD import via `ad_export` / `entra_export` / `primary` purpose-tag aliases, resolve HR master via `hr_master` / `hr_roster` / `supporting` aliases, `load_csv_table` both, call the pure matcher, build summary ("N orphan accounts with no HR record" / "No orphan accounts across X AD rows and Y HR rows"), JSON detail with all counters, `RuleOutcome::base(..., "User access review", "access-review", ..., "AD export")` + `supporting_import = Some(hr_import)` + AD counters + new `hr_rows_considered`.
- `RuleOutcome` gains `hr_rows_considered: Option<i64>`. `MatcherRunResult` and `app/src/lib/api/tauri.ts`'s TypeScript mirror gain the same field, slotted alongside `leaver_rows_considered` in the UAR counters block. Deliberately kept separate from `leaver_rows_considered` — the two supporting-import row counts carry different auditor-facing semantics ("active roster" vs "known terminations") and conflating them would lose that meaning.
- Two integration tests cover the happy and pass paths — orphan-flagged with one exception (plus a disabled row to verify it's skipped from evaluation but still counted in `ad_rows_considered`) and every-AD-row-has-HR-match. The first asserts the detail JSON contains both `"orphan_accounts"` and `"hr_rows_considered"`, that `ActivityLog.matcher_run` fires once, that the Test flips to `in_review`, and that `supporting_import_filename` echoes back `"hr.csv"`. Both assert the new `rule: "orphan_accounts"` discriminator.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/{EngagementDetail,WorkingPaper}.svelte`):
- `MatcherRunResult.hr_rows_considered: number | null` mirrors the Rust field.
- `MATCHER_ENABLED_CODES` gets `"UAM-T-004"` in both routes. EngagementDetail's upload dropdown gains an `hr_master` `PurposeTag` + `PURPOSE_OPTIONS` entry ("HR master roster — Authoritative list of current employees (for orphan-account checks)") so auditors can tag the file when they upload it. The button still sends `overrides: null`; picker UI stays deferred.
- No new Working Paper counters surfaced — the existing UI exposes `rule`, `outcome`, and `exception_count` as the human-visible summary; detailed counters live in the JSON detail blob and the downloadable report.

**Verified**:
- `cargo test --lib` — 104 passing (up from 96). New: six matcher unit tests in `matcher::access_review::tests` and two integration tests in `commands::testing::tests` (`run_matcher_orphan_accounts_flags_ad_rows_with_no_hr_match` + `run_matcher_orphan_accounts_passes_when_all_ad_rows_have_hr_match`). `clone_library_control_shares_risks_between_sibling_controls` updated to expect `test_count == 4` (UAM-C-002 now carries T-002 + T-003 + T-004 + an inherited test).
- `npm run check` — 94 files, 0 errors, 0 warnings.
- `npm run build` — clean vite bundle (~131 KB gzip 41 KB).

**Gotcha worth preserving**: `OrphanReport.ad_rows_considered` counts *every* AD row, including disabled and unmatchable ones — the `ad_rows_skipped_*` buckets are breakdowns of that total, not exclusions from it. Same semantics as the other two UAR reports. The first version of `run_matcher_orphan_accounts_flags_ad_rows_with_no_hr_match` asserted `Some(3)` (only enabled rows) and had to be corrected to `Some(4)` — the same off-by-one trap flagged in the change-management gotcha last entry.

**Deliberately deferred**:
- Override picker UI. Still `null`.
- Per-engagement HR-master staleness threshold (right now any HR master the user uploads is treated as current; no "this roster is older than N days" warning).
- UAM-T-002 (periodic recertification) remains un-wired. Needs its own matcher module — current intent is to build it from a quarter-over-quarter diff of HR or AD snapshots rather than a single-file reconciliation. Not on the immediate path.

**Files of note**: `app/src-tauri/src/matcher/access_review.rs`, `app/src-tauri/resources/library/v0.3.0.json`, `app/src-tauri/resources/library/v0.3.0.json.sig`, `app/src-tauri/src/library/loader.rs`, `app/src-tauri/src/commands/testing.rs`, `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`.

### 2026-04-24 — Change-management + backup matchers wired, dispatcher generalised

Two more rule matchers ship — `CHG-T-001` (approval-before-deployment) and `BKP-T-001` (backup performance) — and the access-review-shaped dispatcher becomes generic across every matcher family. The matcher modules themselves were already built and unit-tested; this change plumbs them into the command layer and the UI's run-matcher button.

**Generic dispatcher** (`app/src-tauri/src/commands/testing.rs`):
- `AccessReviewRule` → `MatcherRule` enum with four variants: `UarTerminatedButActive`, `UarDormantAccounts`, `ChgApprovalBeforeDeployment`, `BkpPerformance`. `for_test_code(&str)` still returns `AppError::Message` for any test code without a wired rule.
- `RunAccessReviewInput { test_id, ad_import_id, leavers_import_id }` → `RunMatcherInput { test_id, overrides: Option<HashMap<String, String>> }`. The overrides map is keyed by `purpose_tag` and pins a specific `DataImport.id`; `null` falls back to the newest matching import per tag. `"primary"` / `"supporting"` work as alias keys so callers that don't know a rule's purpose tags can pin imports positionally.
- `AccessReviewRunResult` → `MatcherRunResult`. Every family-specific counter is now `Option<i64>`. The `ad_import_*` / `leavers_import_*` fields generalise to `primary_import_*` / `supporting_import_*`. CHG counters (`changes_considered`, `changes_skipped_standard`, `changes_skipped_cancelled`, `changes_skipped_not_deployed`, `changes_skipped_no_id`, `changes_skipped_unparseable_dates`) and BKP counters (`jobs_considered`, `jobs_skipped_no_id`, `jobs_skipped_unknown_status`) sit alongside the existing UAR counters.
- `engagement_run_access_review` Tauri command → `engagement_run_matcher`. Per-rule helpers (`run_uar_terminated_but_active`, `run_uar_dormant_accounts`, `run_chg_approval_before_deployment`, `run_bkp_performance`) own import resolution (purpose-tag lookup + blob read + CSV parse). The shared path after dispatch handles the Test + firm guard, blob write of the JSON report, and the `TestResult` / `SyncRecord` / `ChangeLog` / `ActivityLog` / `Evidence` persistence — unchanged in shape from the UAR version.
- `RuleOutcome` carries downstream-relevant fields for every family: `family_label` ("User access review" / "Change management" / "Backup") lands in the ActivityLog line, `report_kind_slug` ("access-review" / "change-management" / "backup") in the blob filename, `primary_import_label` ("AD export" / "Change log" / "Backup log") in the `TestResult.population_ref_label`. A `RuleOutcome::base(...)` constructor defaults every family-specific counter to `None` so each per-rule helper only sets what its rule actually computes.

**Per-rule purpose-tag conventions**:
- UAR terminated-but-active (`UAM-T-001`): `ad_export` or `entra_export` (primary) + `hr_leavers` (supporting).
- UAR dormant-accounts (`UAM-T-003`): `ad_export` or `entra_export` (primary); no supporting.
- CHG approval-before-deployment (`CHG-T-001`): `change_log` or `change_register` (primary); no supporting.
- BKP performance (`BKP-T-001`): `backup_log` or `backup_register` (primary); no supporting.

**Wire-up** (`app/src-tauri/src/lib.rs`, `app/src-tauri/src/commands/{evidence,findings}.rs`):
- Command registration updated. `evidence.rs` and `findings.rs` test imports follow the renamed types (`RunMatcherInput`, `run_matcher`); their four call sites now send `overrides: None`. The module-level doc comment in `evidence.rs` that described matcher-report provenance now names `run_matcher` instead of `run_access_review`.

**Frontend** (`app/src/lib/api/tauri.ts`, `app/src/lib/routes/{EngagementDetail,WorkingPaper}.svelte`):
- `RunMatcherInput` / `MatcherRunResult` TypeScript types mirror the new Rust shapes (all family counters `number | null`). `engagementRunMatcher` replaces `engagementRunAccessReview`.
- `MATCHER_ENABLED_CODES` in both routes: `["UAM-T-001", "UAM-T-003", "CHG-T-001", "BKP-T-001"]`. The button still passes `overrides: null` — picker UI for pinning specific imports stays deferred until an auditor asks to re-run against an older file.
- Upload dropdown's `PurposeTag` + `PURPOSE_OPTIONS` in EngagementDetail gain `change_log` and `backup_log` so auditors can tag those files when they arrive.

**Verified**:
- `cargo test --lib` — 96 passing (up from 73). New: four integration tests in `commands::testing::tests` covering CHG happy + sad paths and BKP happy + sad paths, plus the existing 15 unit tests in `matcher::change_management::tests` and 8 in `matcher::backup::tests` that landed when those matcher modules were first written.
- `npm run check` — 94 files, 0 errors, 0 warnings.
- `npm run build` — clean vite bundle.

**Gotcha worth preserving**: `change_management::tests::changes_considered` counts *all rows in the change register*, not just the in-scope rows. In-scope = `changes_considered` minus each `changes_skipped_*` bucket. The integration test initially asserted in-scope semantics and had to be corrected after the fact — same shape of off-by-one likely waiting in the other families if future tests ever reason about "how many rows did the rule actually evaluate".

**Deliberately deferred**:
- Override picker UI. Backend accepts an arbitrary purpose-tag → import map; the UI still sends `null`. A per-rule picker slots in when auditors start wanting "run this test against last quarter's export instead of the latest".
- Orphan-accounts UAR rule. Matcher isn't built yet; when it lands it adds one more variant to `MatcherRule` and one library test procedure, no dispatcher changes.
- Per-engagement policy config. CHG + BKP currently use the hardcoded thresholds baked into their matcher modules (same posture as UAM-T-003's 90-day dormancy); a per-engagement override table arrives when a firm asks for a different window.

**Files of note**: `app/src-tauri/src/commands/testing.rs`, `app/src-tauri/src/commands/evidence.rs`, `app/src-tauri/src/commands/findings.rs`, `app/src-tauri/src/lib.rs`, `app/src/lib/api/tauri.ts`, `app/src/lib/routes/EngagementDetail.svelte`, `app/src/lib/routes/WorkingPaper.svelte`.

### 2026-04-24 — Working Paper view + CCCER finding editor

Auditors spend their day inside a single test. EngagementDetail gave a bird's-eye
view across all controls and tests; it didn't give the focused surface where
judgment actually happens. The Working Paper route fixes that — it opens one
test at a time with its control context, objective, run history as a timeline,
CCCER-rendered findings, and test-scoped evidence. The CCCER split (Condition,
Criteria, Cause, Effect, Recommendation) replaces the MVP's flat condition /
recommendation pair.

**Migration 0010** (`app/src-tauri/src/db/migrations/0010_cccer_finding_fields.sql`):
- Three `ALTER TABLE Finding ADD COLUMN` statements for `criteria_text`,
  `cause_text`, `effect_text`. All nullable. Existing draft findings remain
  valid — the three new columns start NULL and pick up content the first time
  an auditor opens the finding in the CCCER editor.
- No data backfill. The original `condition_text` and `recommendation_text`
  still come from the matcher-generated elevation boilerplate; CCCER adds the
  three judgmental pieces on top rather than replacing them.

**Rust (`app/src-tauri/src/commands/findings.rs`)**:
- `UpdateFindingInput` and `FindingSummary` gained `criteria_text`, `cause_text`,
  `effect_text` alongside the existing fields. `elevate_finding` sets them to
  `None` on creation (matchers can't infer these honestly; the auditor decides).
- `update_finding`'s change detector now diffs seven fields instead of four;
  ChangeLog records one row per changed field, and an edit of all three new
  fields plus the two old ones produces six field-level entries on top of the
  elevation's whole-row entry.
- `list_findings` SELECT and row mapping updated to project all three new
  columns. Schema-adjacent callers (`ExistingFinding`) track them identically.

**Router / routes** (`app/src/lib/stores/router.ts`, `app/src/App.svelte`):
- Added `"working-paper"` to `RouteId`, added `currentTestId` writable, and
  added `openWorkingPaper(engagementId, testId)` helper. Single slot per
  detail view — same pattern as `currentEngagementId`. No URL routing; Tauri
  has no address bar to reconcile with.
- `App.svelte` picks up a new branch that renders `WorkingPaper.svelte` when
  the route is `"working-paper"`.

**New route (`app/src/lib/routes/WorkingPaper.svelte`)**:
- Loads the engagement's tests, results, findings, severities, and evidence
  via the existing list commands in parallel, then filters client-side to
  `currentTestId`. No new backend aggregate command — keeps the surface
  small; an aggregate can land later if the engagement-wide lists grow
  expensive.
- Sections: Control context card, Test card (objective + metadata + matcher
  button if rule-based), Results timeline (newest first, first item accent-
  coloured), Findings rendered as CCCER `<dl>` cards with "Not yet recorded"
  placeholders on empty criteria/cause/effect, and Evidence table scoped to
  this test (matches `Evidence.test_id` OR `TestEvidenceLink.test_id`).
- Evidence upload form pre-fills `test_id`, so uploads from inside a working
  paper bind to the current test automatically.

**Shared editor (`app/src/lib/components/FindingEditor.svelte`)**:
- Pulled the CCCER form into a component so EngagementDetail and WorkingPaper
  share one editor. Props: `finding`, `severities`, `onSaved`, `onCancel`.
  Callers mount a fresh instance per finding (keyed on `editingFindingId`),
  so the `$state(untrack(() => finding.x))` pattern seeds initial values from
  props without re-binding on prop updates.
- Hint text under each textarea names what auditors should write (criteria =
  standard / policy / control objective; cause = root cause, not symptom;
  effect = concrete exposure). Aim is to guide without blocking.

**EngagementDetail rewiring**:
- Tests table gained an "Open" link per row that calls
  `openWorkingPaper(engagement.id, t.id)`. "Run matcher" stays on the row for
  rule-based tests so the table still works as a quick-run surface.
- Findings table swapped its inline 2-field edit form for the shared
  `FindingEditor`, so editing from either surface produces the same CCCER
  structure. The Findings table's Edit action now sits next to an "Open" link
  that jumps to the related test's working paper (when the finding is tied to
  a test).

**TypeScript (`app/src/lib/api/tauri.ts`)**:
- `FindingSummary` and `UpdateFindingInput` both gained `criteria_text`,
  `cause_text`, `effect_text`.

**Verified**:
- `cargo test --lib` — 73 tests pass (one new: `update_finding_persists_cccer_fields`).
  Previously passing tests that construct `UpdateFindingInput` were updated
  to supply the three new fields as `None`.
- `npm run check` — 0 errors, 0 warnings across 94 files.
- `npm run build` — clean vite build.

**Deliberately deferred**:
- Attach-evidence-to-finding UI. The backend commands are wired and tested,
  but the CCCER editor doesn't surface an evidence picker yet — that lands
  next so we can design the picker against the new CCCER layout rather than
  retrofit it.
- Review-note annotations on the working paper. The route is the surface
  that will host them; the inline-margin review-note pattern is its own
  design problem.
- Per-test aggregate backend command. Today the working paper reloads all
  engagement-wide lists and filters client-side — cheap while engagements
  stay small. Revisit if a single engagement grows beyond a few hundred
  tests / results.

**Files of note**: `app/src-tauri/src/db/migrations/0010_cccer_finding_fields.sql`,
`app/src-tauri/src/db/migrations/mod.rs`, `app/src-tauri/src/commands/findings.rs`,
`app/src/lib/stores/router.ts`, `app/src/App.svelte`,
`app/src/lib/components/FindingEditor.svelte`,
`app/src/lib/routes/WorkingPaper.svelte`, `app/src/lib/routes/EngagementDetail.svelte`,
`app/src/lib/api/tauri.ts`.

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
