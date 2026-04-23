# Audit Application — Data Model (v0.1)

First-pass schema for the foundation and core-domain modules. Expressed in SQLite-flavoured SQL-ish notation since the storage layer is SQLCipher-encrypted SQLite. Types shown as `TEXT`, `INTEGER`, `BLOB`, `REAL`. Booleans are stored as `INTEGER 0/1`. Timestamps are Unix epoch seconds (UTC).

This file covers **Modules 1, 2, 3, 4, 5, 6, 7, 8, 9, 12**. Modules 10, 11, 13 will be added as their designs firm up (see `PROGRESS.md`).

For module rationale and relationships see `MODULES.md`. For longer-form decision rationale see `NOTES.md`.

---

## Conventions

- **Primary keys**: UUID v7 stored as `TEXT` (time-sortable, index-friendly).
- **Foreign keys**: shown as `FK → Table.column`. Enforced at application layer; SQLite FK enforcement enabled at connection open.
- **Timestamps**: `INTEGER` Unix epoch seconds, UTC. Never store local time.
- **Soft delete**: `deleted` boolean lives on `SyncRecord`, not per entity — one place to check, one place to filter.
- **JSON fields**: suffix `_json`, stored as `TEXT`. Used for list-of-strings, sparse metadata, and field-level override maps. Structured fields get their own columns.
- **Encrypted payloads**: anything larger than ~4KB or containing free-form prose routes through `EncryptedBlob` (separate file on disk, AES-256-GCM). Small structured fields stay inline; SQLCipher encrypts the DB page itself.
- **Every mutable row has a matching `SyncRecord`** keyed by the row's `id`. No `SyncRecord`, not synced.
- **Library entries are immutable per version**. Updates create new rows with a new `library_version`, not UPDATE-in-place.

---

## Module 12 — Sync & Storage

Foundation. Everything else references it.

```
SyncRecord
  id                  TEXT PK              -- same UUID as the entity row it tracks
  entity_type         TEXT NOT NULL        -- e.g. "Engagement", "Test", "Finding"
  entity_id           TEXT NOT NULL        -- redundant with id; keeps queries explicit
  last_modified_at    INTEGER NOT NULL
  last_modified_by    TEXT FK → User.id
  version             INTEGER NOT NULL     -- monotonic; bumped on every mutation
  deleted             INTEGER NOT NULL     -- 0 | 1
  sync_state          TEXT NOT NULL        -- local_only | pending_upload | synced | conflict
  remote_version      INTEGER              -- last version confirmed by server
  INDEX (entity_type, entity_id)
  INDEX (sync_state)

ChangeLog
  id                  TEXT PK
  sync_record_id      TEXT FK → SyncRecord.id
  occurred_at         INTEGER NOT NULL
  user_id             TEXT FK → User.id
  field_name          TEXT NOT NULL        -- "." for whole-row create/delete
  old_value_json      TEXT                 -- null on create
  new_value_json      TEXT                 -- null on delete
  INDEX (sync_record_id, occurred_at)

ConflictResolution
  id                  TEXT PK
  sync_record_id      TEXT FK → SyncRecord.id
  detected_at         INTEGER NOT NULL
  local_version       INTEGER NOT NULL
  remote_version      INTEGER NOT NULL
  resolution          TEXT                 -- keep_local | keep_remote | manual_merge | pending
  resolved_by         TEXT FK → User.id
  resolved_at         INTEGER

EncryptedBlob
  id                  TEXT PK
  owning_entity_type  TEXT                 -- nullable; null for orphan-uploads staged before link
  owning_entity_id    TEXT
  filename            TEXT
  mime_type           TEXT
  nonce               BLOB NOT NULL        -- 12 bytes, AES-GCM
  ciphertext_path     TEXT NOT NULL        -- on-disk path; ciphertext itself not in DB
  auth_tag            BLOB NOT NULL        -- 16 bytes
  plaintext_size      INTEGER NOT NULL
  key_id              TEXT FK → KeychainEntry.id
  sha256_plaintext    TEXT                 -- for dedup and integrity
  created_at          INTEGER NOT NULL
  INDEX (owning_entity_type, owning_entity_id)
  INDEX (sha256_plaintext)

KeychainEntry
  id                  TEXT PK
  purpose             TEXT NOT NULL        -- engagement_master | file_kek | license | sync_session
  scope_entity_type   TEXT                 -- e.g. "Engagement" when engagement-scoped
  scope_entity_id     TEXT
  os_keychain_ref     TEXT NOT NULL        -- service + account string for OS keyring lookup
  wrapped_key         BLOB                 -- backup copy wrapped by firm master (optional)
  algorithm           TEXT NOT NULL        -- "AES-256-GCM"
  kdf                 TEXT                 -- "Argon2id" when derived
  kdf_params_json     TEXT
  created_at          INTEGER NOT NULL
  rotated_from        TEXT FK → KeychainEntry.id   -- lineage on key rotation
```

**Notes:**
- The encrypted file itself lives at `ciphertext_path` on disk (e.g., `~/Library/Application Support/Audit/engagements/<eng_id>/blobs/<blob_id>.bin`). Keeping ciphertext out of the DB keeps the DB file small and backups efficient.
- `sha256_plaintext` enables deduplication and later prior-year evidence matching. Hash is computed *before* encryption, so duplicates collapse to one physical file regardless of nonce.
- Key rotation creates a new `KeychainEntry` with `rotated_from` pointing at the retired entry. Old ciphertexts stay addressable until re-encryption runs.

---

## Module 1 — Identity & Licensing

```
Firm
  id                  TEXT PK
  name                TEXT NOT NULL
  country             TEXT NOT NULL        -- ISO 3166-1 alpha-2
  default_locale      TEXT NOT NULL        -- e.g. "en-GB"
  license_id          TEXT FK → License.id
  library_version     TEXT                 -- currently-applied library revision
  settings_json       TEXT                 -- firm-wide prefs (accent colour, logo ref, etc.)
  created_at          INTEGER NOT NULL

User
  id                  TEXT PK
  firm_id             TEXT FK → Firm.id
  email               TEXT NOT NULL UNIQUE
  display_name        TEXT NOT NULL
  role_id             TEXT FK → Role.id
  argon2_hash         TEXT NOT NULL        -- password verifier; never the password itself
  master_key_wrapped  BLOB NOT NULL        -- user's master key, wrapped by Argon2id-derived KEK
  status              TEXT NOT NULL        -- active | suspended | deleted
  last_seen_at        INTEGER
  created_at          INTEGER NOT NULL

Role
  id                  TEXT PK
  firm_id             TEXT FK → Firm.id    -- null for built-in roles
  name                TEXT NOT NULL        -- Partner | Manager | Senior | Associate | Admin | ReadOnly
  permissions_json    TEXT NOT NULL        -- flat list of permission strings
  is_builtin          INTEGER NOT NULL

License
  id                  TEXT PK
  firm_id             TEXT FK → Firm.id
  tier                TEXT NOT NULL        -- subscription | prepaid | byo_key
  seats               INTEGER NOT NULL
  hardware_binding    TEXT                 -- hash of machine fingerprint (null = not yet bound)
  issued_at           INTEGER NOT NULL
  expires_at          INTEGER NOT NULL
  grace_until         INTEGER              -- offline-use grace window
  signature           BLOB NOT NULL        -- ed25519 sig over canonical JSON body by Simba's key
  last_validated_at   INTEGER

SubscriptionPlan
  id                  TEXT PK
  license_id          TEXT FK → License.id
  monthly_quota_tokens INTEGER NOT NULL
  tokens_used_cycle   INTEGER NOT NULL
  cycle_resets_at     INTEGER NOT NULL
  overage_policy      TEXT NOT NULL        -- hard_block | upgrade_prompt | metered

PrepaidBalance
  id                  TEXT PK
  license_id          TEXT FK → License.id
  tokens_remaining    INTEGER NOT NULL
  last_topup_at       INTEGER
  last_topup_tokens   INTEGER

BYOKeyConfig
  id                  TEXT PK
  user_id             TEXT FK → User.id
  provider            TEXT NOT NULL        -- claude | openai | gemini | meta | ollama
  key_label           TEXT NOT NULL        -- user-facing nickname
  key_ciphertext      BLOB NOT NULL        -- encrypted with user master key
  nonce               BLOB NOT NULL
  auth_tag            BLOB NOT NULL
  created_at          INTEGER NOT NULL
  last_used_at        INTEGER
```

**Notes:**
- `argon2_hash` is the verifier used to re-derive the KEK that unwraps `master_key_wrapped`. The password itself is never stored or transmitted. Changing the password = rewrap the master key, never re-encrypt underlying data.
- `License.signature` is verified offline at launch using Simba's bundled public key. Tamper with any field → signature fails → app refuses to open engagement data.
- `BYOKeyConfig.key_ciphertext` is encrypted with the *user's* master key, not the engagement key. BYO keys follow the user across engagements.

---

## Module 2 — Client Management

```
Client
  id                  TEXT PK
  firm_id             TEXT FK → Firm.id
  name                TEXT NOT NULL
  industry_id         TEXT FK → Industry.id
  country             TEXT NOT NULL
  status              TEXT NOT NULL        -- active | archived
  created_at          INTEGER NOT NULL

ClientContact
  id                  TEXT PK
  client_id           TEXT FK → Client.id
  name                TEXT NOT NULL
  role                TEXT
  email               TEXT
  phone               TEXT
  is_portal_enabled   INTEGER NOT NULL     -- 1 = can be invited to client portal
  notes_blob_id       TEXT FK → EncryptedBlob.id

Industry
  id                  TEXT PK
  name                TEXT NOT NULL        -- Banking | Insurance | Retail | Public Sector | ...
  default_system_types_json TEXT           -- ["AD","SAP","core_banking"] — prefill on engagement scoping
  is_builtin          INTEGER NOT NULL

ClientSettings
  id                  TEXT PK
  client_id           TEXT FK → Client.id
  evidence_retention_years INTEGER NOT NULL DEFAULT 7
  portal_logo_blob_id TEXT FK → EncryptedBlob.id
  portal_accent_colour TEXT                -- hex; overrides firm default for this client's portal view
  custom_fields_json  TEXT
```

---

## Module 3 — Engagement Core

The container everything else lives inside. An engagement has its own encryption key (engagement master), wrapped by the user master. One compromise ≠ full corpus compromise.

```
Engagement
  id                       TEXT PK
  client_id                TEXT FK → Client.id
  name                     TEXT NOT NULL        -- e.g. "FY2026 ITGC Audit"
  period_id                TEXT FK → EngagementPeriod.id
  status_id                TEXT FK → EngagementStatus.id
  prior_engagement_id      TEXT FK → Engagement.id   -- for carry-forward lineage
  library_version_at_start TEXT NOT NULL
  encryption_key_id        TEXT FK → KeychainEntry.id   -- engagement master key
  lead_partner_id          TEXT FK → User.id
  created_at               INTEGER NOT NULL
  closed_at                INTEGER
  archive_bundle_blob_id   TEXT FK → EncryptedBlob.id   -- populated on close

EngagementPeriod
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  start_date          TEXT NOT NULL        -- ISO 8601 date
  end_date            TEXT NOT NULL
  fiscal_year_label   TEXT NOT NULL        -- "FY2026", "2025/26"

EngagementTeam
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  user_id             TEXT FK → User.id
  team_role           TEXT NOT NULL        -- partner | manager | senior | associate | reviewer
  assigned_at         INTEGER NOT NULL
  unassigned_at       INTEGER
  UNIQUE (engagement_id, user_id)

EngagementScope
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  scope_statement_blob_id TEXT FK → EncryptedBlob.id
  approved_by         TEXT FK → User.id
  approved_at         INTEGER

EngagementBudget
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  total_hours         REAL NOT NULL
  hours_by_role_json  TEXT                 -- {"partner": 10, "manager": 40, ...}
  actual_hours        REAL NOT NULL DEFAULT 0    -- rolled up from time tracking

EngagementStatus
  id                  TEXT PK
  name                TEXT NOT NULL        -- Planning | Fieldwork | Review | Reporting | Closed
  sort_order          INTEGER NOT NULL
  is_terminal         INTEGER NOT NULL     -- 1 for Closed
  is_builtin          INTEGER NOT NULL
```

**Notes:**
- `encryption_key_id` enables per-engagement keying. When a user leaves the firm, revoke by rotating engagement keys for in-flight engagements rather than re-encrypting everything.
- `archive_bundle_blob_id` is the immutable close-out bundle described in `MODULES.md` (Activity log retention decision). Populated once, never mutated.
- `prior_engagement_id` is the walk-back root for the `derived_from` chains on `EngagementRisk`, `EngagementControl`, `Test`.

---

## Module 4 — System Inventory

```
System
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  name                TEXT NOT NULL        -- "SAP ECC Production"
  type                TEXT NOT NULL        -- SAP | Oracle_EBS | AD | Entra | SQL | Oracle_DB | core_banking | custom
  template_id         TEXT FK → SystemTemplate.id
  environment         TEXT NOT NULL        -- prod | uat | dev
  criticality         TEXT NOT NULL        -- high | medium | low
  business_owner      TEXT
  it_owner            TEXT
  metadata_json       TEXT                 -- versions, hostnames, URLs
  derived_from        TEXT FK → System.id  -- prior-engagement system this one was cloned from
  created_at          INTEGER NOT NULL

SystemTemplate
  id                  TEXT PK
  name                TEXT NOT NULL        -- "SAP ECC", "Oracle EBS", "Active Directory", "Temenos T24"
  type                TEXT NOT NULL        -- same enum as System.type
  default_control_ids_json TEXT            -- [LibraryControl.id, ...]
  default_test_ids_json    TEXT            -- [TestProcedure.id, ...]
  ui_hints_json       TEXT                 -- per-system UI guidance shown during scoping
  library_version     TEXT NOT NULL
  is_builtin          INTEGER NOT NULL

CustomSystem
  id                  TEXT PK
  system_id           TEXT FK → System.id
  architecture_notes_blob_id TEXT FK → EncryptedBlob.id
  data_flow_diagram_blob_id  TEXT FK → EncryptedBlob.id
```

**Notes:**
- `System.derived_from` carries system-level lineage across years so the new engagement's AD inherits the prior year's ownership annotations, risk ratings, and notes unless overridden.
- `SystemTemplate` is library-owned; versioned by `library_version`. Firms override via `FirmOverride` (Module 5).

---

## Module 5 — Risk & Control Library

The methodology layer. Library entries are dev-shipped. Firm-level changes live in `FirmOverride` so library updates don't clobber them.

```
LibraryRisk
  id                  TEXT PK
  code                TEXT NOT NULL        -- "R-ACC-001"
  title               TEXT NOT NULL
  description         TEXT NOT NULL
  applicable_system_types_json TEXT        -- ["AD","SAP","SQL"]
  default_inherent_rating TEXT             -- high | medium | low
  library_version     TEXT NOT NULL
  superseded_by       TEXT FK → LibraryRisk.id
  UNIQUE (code, library_version)

LibraryControl
  id                  TEXT PK
  code                TEXT NOT NULL        -- "C-ACC-001"
  title               TEXT NOT NULL
  description         TEXT NOT NULL
  objective           TEXT NOT NULL
  applicable_system_types_json TEXT
  control_type        TEXT NOT NULL        -- preventive | detective | corrective
  frequency           TEXT                 -- continuous | daily | weekly | monthly | quarterly | annual | ad_hoc
  related_risk_ids_json TEXT
  library_version     TEXT NOT NULL
  superseded_by       TEXT FK → LibraryControl.id
  UNIQUE (code, library_version)

TestProcedure
  id                  TEXT PK              -- library template; Test instance lives in Module 6
  control_id          TEXT FK → LibraryControl.id
  code                TEXT NOT NULL        -- "T-ACC-001"
  name                TEXT NOT NULL
  objective           TEXT NOT NULL
  steps_json          TEXT NOT NULL        -- ordered procedure steps
  expected_evidence_checklist_id TEXT FK → ExpectedEvidenceChecklist.id
  sampling_default    TEXT NOT NULL        -- full_population | statistical_mus | statistical_attribute | judgemental | none
  automation_hint     TEXT NOT NULL        -- rule_based | classical_ml | local_llm | hosted_llm | manual
  library_version     TEXT NOT NULL
  UNIQUE (code, library_version)

FrameworkMapping
  id                  TEXT PK
  entity_type         TEXT NOT NULL        -- library_risk | library_control | test_procedure
  entity_id           TEXT NOT NULL
  framework           TEXT NOT NULL        -- cobit_2019 | nist_csf | iso_27001 | pci
  reference           TEXT NOT NULL        -- e.g. "DSS05.04"
  library_version     TEXT NOT NULL
  INDEX (entity_type, entity_id)
  INDEX (framework, reference)

ExpectedEvidenceChecklist
  id                  TEXT PK
  test_procedure_id   TEXT FK → TestProcedure.id
  items_json          TEXT NOT NULL        -- [{"label":"user listing","is_required":true}, ...]
  library_version     TEXT NOT NULL

FirmOverride
  id                  TEXT PK
  firm_id             TEXT FK → Firm.id
  base_entity_type    TEXT NOT NULL        -- library_risk | library_control | test_procedure | system_template
  base_entity_code    TEXT NOT NULL        -- use code+version, not id — survives library updates
  base_library_version TEXT NOT NULL       -- version the override was authored against
  override_json       TEXT NOT NULL        -- field-level overrides, sparse
  disabled            INTEGER NOT NULL     -- 1 = hide from this firm entirely
  created_by          TEXT FK → User.id
  created_at          INTEGER NOT NULL
  INDEX (firm_id, base_entity_type, base_entity_code)
```

**Notes:**
- Library updates arrive as versioned bundles. Shipping a new library version inserts new rows (new `id` per row) and sets `superseded_by` on the prior version's rows — history preserved.
- `FirmOverride` keys off `code + library_version`, not `id`. When a library update supersedes a control, the override is re-evaluated against the new version (either still applies, or surfaces a conflict for the firm to review).
- `automation_hint` is the tier ladder from `NOTES.md` — rule-based first, classical ML second, local LLM third, hosted LLM last. Honest label so auditors know what will run.

---

## Module 6 — Fieldwork & Testing

Engagement-level clones of library methodology plus the machinery for executing tests: samples, results, imports, connectors.

```
EngagementRisk
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  derived_from                 TEXT FK → LibraryRisk.id      -- null if hand-authored
  source_library_version       TEXT                           -- null if hand-authored
  prior_engagement_risk_id     TEXT FK → EngagementRisk.id   -- carry-forward lineage
  code                         TEXT NOT NULL                  -- copied from library; editable
  title                        TEXT NOT NULL
  description                  TEXT NOT NULL
  inherent_rating              TEXT NOT NULL                  -- high | medium | low
  residual_rating              TEXT                           -- set after testing
  applicable_system_ids_json   TEXT                           -- [System.id, ...] — scoping within engagement
  notes_blob_id                TEXT FK → EncryptedBlob.id
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  UNIQUE (engagement_id, code)
  INDEX (engagement_id)

EngagementControl
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  derived_from                 TEXT FK → LibraryControl.id
  source_library_version       TEXT
  prior_engagement_control_id  TEXT FK → EngagementControl.id
  code                         TEXT NOT NULL
  title                        TEXT NOT NULL
  description                  TEXT NOT NULL
  objective                    TEXT NOT NULL
  control_type                 TEXT NOT NULL                  -- preventive | detective | corrective
  frequency                    TEXT
  design_assessment            TEXT                           -- designed_effective | designed_deficient | not_assessed
  operating_assessment         TEXT                           -- effective | deficient | not_tested
  related_engagement_risk_ids_json TEXT
  applicable_system_ids_json   TEXT
  notes_blob_id                TEXT FK → EncryptedBlob.id
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  UNIQUE (engagement_id, code)
  INDEX (engagement_id)

Test
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  engagement_control_id        TEXT FK → EngagementControl.id
  system_id                    TEXT FK → System.id           -- which system this instance targets
  derived_from                 TEXT FK → TestProcedure.id
  source_library_version       TEXT
  prior_test_id                TEXT FK → Test.id             -- prior engagement's same test
  code                         TEXT NOT NULL
  name                         TEXT NOT NULL
  objective                    TEXT NOT NULL
  steps_json                   TEXT NOT NULL                  -- copy of procedure steps, editable per-engagement
  automation_tier              TEXT NOT NULL                  -- rule_based | classical_ml | local_llm | hosted_llm | manual
  assigned_to                  TEXT FK → User.id
  status                       TEXT NOT NULL                  -- not_started | in_progress | blocked | complete
  planned_start_date           TEXT                           -- ISO 8601
  planned_end_date             TEXT
  actual_started_at            INTEGER
  actual_completed_at          INTEGER
  notes_blob_id                TEXT FK → EncryptedBlob.id
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  UNIQUE (engagement_id, code, system_id)
  INDEX (engagement_id, status)
  INDEX (engagement_control_id)

SamplingPlan
  id                           TEXT PK
  test_id                      TEXT FK → Test.id
  method                       TEXT NOT NULL                  -- full_population | statistical_mus | statistical_attribute | judgemental
  population_size              INTEGER NOT NULL
  sample_size                  INTEGER NOT NULL
  confidence_level             REAL                           -- e.g. 0.95
  tolerable_rate               REAL                           -- attribute sampling
  expected_error_rate          REAL
  materiality                  REAL                           -- MUS
  seed                         INTEGER NOT NULL               -- recorded for reviewer reproducibility
  parameters_json              TEXT                           -- method-specific overflow (strata, interval, etc.)
  drawn_by                     TEXT FK → User.id
  drawn_at                     INTEGER NOT NULL
  INDEX (test_id)

Sample
  id                           TEXT PK
  sampling_plan_id             TEXT FK → SamplingPlan.id
  test_id                      TEXT FK → Test.id              -- redundant but enables direct lookup
  ordinal                      INTEGER NOT NULL               -- 1-based within the plan
  population_ref               TEXT NOT NULL                  -- opaque ref into source population (e.g. employee_id)
  population_ref_label         TEXT                           -- human-readable summary
  selection_reason             TEXT                           -- "random" | "judgemental_high_risk" | ...
  UNIQUE (sampling_plan_id, ordinal)
  INDEX (test_id)

TestResult
  id                           TEXT PK
  test_id                      TEXT FK → Test.id
  sample_id                    TEXT FK → Sample.id            -- null for population-wide or design-only tests
  outcome                      TEXT NOT NULL                  -- pass | exception | not_applicable | could_not_test
  exception_summary            TEXT                           -- one-line; full detail lives on Finding if elevated
  evidence_count               INTEGER NOT NULL DEFAULT 0
  performed_by                 TEXT FK → User.id
  performed_at                 INTEGER NOT NULL
  notes_blob_id                TEXT FK → EncryptedBlob.id
  INDEX (test_id)
  INDEX (test_id, outcome)

TestConclusion
  id                           TEXT PK
  test_id                      TEXT FK → Test.id UNIQUE       -- one conclusion per test
  conclusion                   TEXT NOT NULL                  -- effective | deficient | compensating_control_relied_on | inconclusive
  rationale_blob_id            TEXT FK → EncryptedBlob.id
  exception_count              INTEGER NOT NULL
  projected_error_rate         REAL                           -- populated for statistical sampling
  reached_by                   TEXT FK → User.id
  reached_at                   INTEGER NOT NULL

Connector
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  system_id                    TEXT FK → System.id
  kind                         TEXT NOT NULL                  -- ldap | sap_rfc | sql_readonly | sftp | rest_api | csv_inbox
  status                       TEXT NOT NULL                  -- configured | connected | error | disabled
  config_json                  TEXT NOT NULL                  -- host, port, bind DN, etc. (secrets in keychain)
  keychain_entry_id            TEXT FK → KeychainEntry.id
  last_connected_at            INTEGER
  last_error                   TEXT
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  INDEX (engagement_id)

DataImport
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  system_id                    TEXT FK → System.id            -- nullable for cross-system imports
  connector_id                 TEXT FK → Connector.id         -- nullable for manual uploads
  source_kind                  TEXT NOT NULL                  -- csv_upload | excel_upload | ldap_query | sap_export | sql_query | pdf_ocr
  filename                     TEXT                           -- for file uploads
  blob_id                      TEXT FK → EncryptedBlob.id     -- raw import payload
  row_count                    INTEGER
  sha256_plaintext             TEXT NOT NULL                  -- matches blob hash; tamper-evident
  schema_json                  TEXT                           -- detected column names + types
  purpose_tag                  TEXT                           -- "ad_user_export" | "hr_leavers" | "journal_entries" — semantic marker
  imported_by                  TEXT FK → User.id
  imported_at                  INTEGER NOT NULL
  INDEX (engagement_id)
  INDEX (engagement_id, purpose_tag)
```

**Notes:**
- Engagement-level clones (`EngagementRisk`, `EngagementControl`, `Test`) carry both `derived_from` (library lineage) and `prior_engagement_*_id` (carry-forward lineage). Walking either chain answers "where did this come from?" without ambiguity.
- One `EngagementControl` can have multiple `Test` rows — one per system in scope. The `UNIQUE (engagement_id, code, system_id)` constraint enforces one test per control-per-system.
- `TestResult.sample_id` is nullable so a population-wide or design-only test records a single result without a synthetic sample.
- `SamplingPlan.seed` is required — the reviewer must be able to reproduce exactly the same selection.
- `Connector.keychain_entry_id` keeps connection secrets out of the DB. Same pattern as the user master key.
- `DataImport.purpose_tag` is the semantic handle matchers use ("grab the `ad_user_export` and the `hr_leavers` from this engagement"). Loose enum by design — firms can add their own tags.

---

## Module 7 — Evidence Management

Everything that backs up a test. Covers PBC (provided-by-client) request lifecycle, chain-of-custody, and cross-year evidence linking.

```
Evidence
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  test_id                      TEXT FK → Test.id              -- primary linked test
  test_result_id               TEXT FK → TestResult.id        -- null unless tied to a specific sample
  blob_id                      TEXT FK → EncryptedBlob.id
  data_import_id               TEXT FK → DataImport.id        -- set when evidence is a slice of a bulk import
  title                        TEXT NOT NULL
  description                  TEXT
  source                       TEXT NOT NULL                  -- client_upload | auditor_extract | connector | prior_year_link | generated
  obtained_at                  INTEGER NOT NULL
  obtained_from                TEXT                           -- contact name or system reference
  pbc_request_id               TEXT FK → PBCRequest.id        -- null if not PBC-driven
  INDEX (engagement_id)
  INDEX (test_id)

TestEvidenceLink
  id                           TEXT PK
  test_id                      TEXT FK → Test.id
  evidence_id                  TEXT FK → Evidence.id
  relevance                    TEXT                           -- primary | supporting | cross_reference
  created_at                   INTEGER NOT NULL
  UNIQUE (test_id, evidence_id)
  INDEX (evidence_id)

EvidenceTag
  id                           TEXT PK
  evidence_id                  TEXT FK → Evidence.id
  tag                          TEXT NOT NULL                  -- "user_listing" | "change_ticket" | "backup_log" | firm-defined
  INDEX (evidence_id)
  INDEX (tag)

EvidenceProvenance
  id                           TEXT PK
  evidence_id                  TEXT FK → Evidence.id
  chain_ordinal                INTEGER NOT NULL               -- 1 = origin; increments on each transformation
  action                       TEXT NOT NULL                  -- uploaded | ocrd | extracted | redacted | prior_year_linked
  actor_type                   TEXT NOT NULL                  -- user | system | portal_user
  actor_id                     TEXT                           -- User.id / PortalUser.id / tool name
  occurred_at                  INTEGER NOT NULL
  detail_json                  TEXT                           -- tool version, source ref, etc.
  UNIQUE (evidence_id, chain_ordinal)

PBCRequest
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  test_id                      TEXT FK → Test.id              -- nullable for engagement-level requests
  title                        TEXT NOT NULL
  description_blob_id          TEXT FK → EncryptedBlob.id
  requested_from_contact_id    TEXT FK → ClientContact.id
  status                       TEXT NOT NULL                  -- draft | sent | partially_received | received | overdue | cancelled
  due_date                     TEXT                           -- ISO 8601
  sent_at                      INTEGER
  received_at                  INTEGER
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  INDEX (engagement_id, status)

PBCStatus
  id                           TEXT PK
  pbc_request_id               TEXT FK → PBCRequest.id
  status                       TEXT NOT NULL
  changed_at                   INTEGER NOT NULL
  actor_type                   TEXT NOT NULL                  -- user | portal_user | system
  actor_id                     TEXT                           -- resolved via actor_type
  note                         TEXT
  INDEX (pbc_request_id, changed_at)

PriorYearEvidenceLink
  id                           TEXT PK
  current_engagement_id        TEXT FK → Engagement.id
  current_test_id              TEXT FK → Test.id
  prior_evidence_id            TEXT FK → Evidence.id
  attestation_blob_id          TEXT FK → EncryptedBlob.id     -- "unchanged from prior year" rationale
  attested_by                  TEXT FK → User.id
  attested_at                  INTEGER NOT NULL
  INDEX (current_test_id)
```

**Notes:**
- `Evidence.test_id` is the **primary** linkage. Use `TestEvidenceLink` for additional associations (one file backing multiple tests). Exactly one primary per evidence item; many secondary.
- `EvidenceProvenance` is append-only chain-of-custody. Client evidence often passes through OCR, extraction, or manual slicing before it lands on a test — the provenance chain makes each transformation visible.
- `PBCStatus` keeps the lifecycle history (when did it move from `sent` to `partially_received`?). Separate from `ActivityLog` because it drives the reminder / overdue UI and the client portal's request dashboard.
- `PriorYearEvidenceLink.attestation_blob_id` holds the auditor's written rationale for why prior-year evidence is still sufficient. Required per the "fresh upload preferred" decision.

---

## Module 8 — Findings & Remediation

Control deficiencies in CCCER form (Condition / Criteria / Cause / Effect / Recommendation), the management response, and year-over-year recurrence tracking.

```
Finding
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  test_id                      TEXT FK → Test.id              -- origin test
  engagement_control_id        TEXT FK → EngagementControl.id -- control the finding is against
  code                         TEXT NOT NULL                  -- "F-<eng-short>-NNN"
  title                        TEXT NOT NULL
  condition_blob_id            TEXT FK → EncryptedBlob.id     -- "what we observed"
  criteria_blob_id             TEXT FK → EncryptedBlob.id     -- "what should have happened"
  cause_blob_id                TEXT FK → EncryptedBlob.id     -- "why it happened"
  effect_blob_id               TEXT FK → EncryptedBlob.id     -- "impact / risk"
  recommendation_blob_id       TEXT FK → EncryptedBlob.id
  severity_id                  TEXT FK → FindingSeverity.id
  root_cause_id                TEXT FK → RootCauseTaxonomy.id
  status                       TEXT NOT NULL                  -- draft | open | management_accepted | in_remediation | remediated | closed | disputed
  identified_by                TEXT FK → User.id
  identified_at                INTEGER NOT NULL
  first_communicated_at        INTEGER
  closed_at                    INTEGER
  UNIQUE (engagement_id, code)
  INDEX (engagement_id, status)
  INDEX (engagement_control_id)

FindingTestResultLink
  id                           TEXT PK
  finding_id                   TEXT FK → Finding.id
  test_result_id               TEXT FK → TestResult.id
  UNIQUE (finding_id, test_result_id)
  INDEX (test_result_id)

FindingSeverity
  id                           TEXT PK
  name                         TEXT NOT NULL                  -- critical | high | medium | low | observation
  sort_order                   INTEGER NOT NULL
  description                  TEXT
  is_builtin                   INTEGER NOT NULL

RootCauseTaxonomy
  id                           TEXT PK
  firm_id                      TEXT FK → Firm.id              -- null for built-in taxonomy
  code                         TEXT NOT NULL                  -- "RC-ACC-01"
  name                         TEXT NOT NULL                  -- "Inadequate user access review"
  category                     TEXT NOT NULL                  -- people | process | technology | governance
  description                  TEXT
  parent_id                    TEXT FK → RootCauseTaxonomy.id -- hierarchical
  is_builtin                   INTEGER NOT NULL
  INDEX (firm_id, parent_id)

ManagementActionPlan
  id                           TEXT PK
  finding_id                   TEXT FK → Finding.id UNIQUE    -- one MAP per finding
  response_type                TEXT NOT NULL                  -- accept | mitigate | transfer | dispute
  response_blob_id             TEXT FK → EncryptedBlob.id     -- management's written response
  action_plan_blob_id          TEXT FK → EncryptedBlob.id     -- remediation steps
  owner_contact_id             TEXT FK → ClientContact.id
  target_date                  TEXT                           -- ISO 8601
  committed_at                 INTEGER                        -- management sign-off timestamp
  status                       TEXT NOT NULL                  -- proposed | agreed | rejected | in_progress | completed | overdue

FollowUp
  id                           TEXT PK
  finding_id                   TEXT FK → Finding.id
  management_action_plan_id    TEXT FK → ManagementActionPlan.id
  follow_up_date               TEXT NOT NULL                  -- ISO 8601
  performed_by                 TEXT FK → User.id
  performed_at                 INTEGER
  verification_outcome         TEXT                           -- verified_complete | partially_complete | not_complete | superseded
  evidence_id                  TEXT FK → Evidence.id          -- supporting evidence
  notes_blob_id                TEXT FK → EncryptedBlob.id
  INDEX (finding_id, follow_up_date)

RecurringFindingLink
  id                           TEXT PK
  current_finding_id           TEXT FK → Finding.id
  prior_finding_id             TEXT FK → Finding.id
  match_type                   TEXT NOT NULL                  -- exact_control | same_root_cause | semantic
  match_confidence             REAL                           -- 0..1 (exact = 1.0; semantic < 1.0)
  identified_at                INTEGER NOT NULL
  confirmed_by                 TEXT FK → User.id              -- auditor confirms auto-detected recurrence
  confirmed_at                 INTEGER
  INDEX (current_finding_id)
  INDEX (prior_finding_id)
```

**Notes:**
- CCCER blobs are held separately so each can be edited, reviewed, and LLM-drafted independently. Drafting cause vs recommendation are different prompts; keeping them split avoids re-prompting the whole finding.
- `FindingTestResultLink` — one finding often aggregates many exception rows (e.g. "12 terminated users still active" = 12 `TestResult`s, one `Finding`). The join makes the aggregation explicit and queryable.
- `ManagementActionPlan` is one-per-finding (UNIQUE). Many `FollowUp`s per plan.
- `RecurringFindingLink.match_type`:
  - `exact_control` — both findings chain back through `EngagementControl.derived_from` to the same library control. Pure rule-based, confidence 1.0.
  - `same_root_cause` — both tagged with the same `RootCauseTaxonomy`. Rule-based.
  - `semantic` — sentence-transformer similarity above a threshold. Auditor-confirmable suggestion; confidence carries through from the matcher.

---

## Module 9 — Working Papers & Review

Structured review documents. Supports per-test, per-section, or custom grouping (the three-format decision in `MODULES.md`).

```
WorkingPaper
  id                           TEXT PK
  engagement_id                TEXT FK → Engagement.id
  code                         TEXT NOT NULL                  -- "WP-<eng-short>-NNN"
  title                        TEXT NOT NULL
  wp_type                      TEXT NOT NULL                  -- per_test | per_section | custom
  section                      TEXT                           -- grouping label (e.g. "Change Management")
  status                       TEXT NOT NULL                  -- draft | prepared | in_review | reviewed | locked
  prepared_by                  TEXT FK → User.id
  prepared_at                  INTEGER
  reviewed_by                  TEXT FK → User.id
  reviewed_at                  INTEGER
  locked_at                    INTEGER
  content_blob_id              TEXT FK → EncryptedBlob.id     -- rendered narrative body (when not split into sections)
  created_by                   TEXT FK → User.id
  created_at                   INTEGER NOT NULL
  UNIQUE (engagement_id, code)
  INDEX (engagement_id, status)

WPSection
  id                           TEXT PK
  working_paper_id             TEXT FK → WorkingPaper.id
  ordinal                      INTEGER NOT NULL
  heading                      TEXT NOT NULL                  -- "Objective" | "Procedures performed" | "Conclusion" | custom
  body_blob_id                 TEXT FK → EncryptedBlob.id
  UNIQUE (working_paper_id, ordinal)

WPTestLink
  id                           TEXT PK
  working_paper_id             TEXT FK → WorkingPaper.id
  test_id                      TEXT FK → Test.id
  include_samples              INTEGER NOT NULL               -- 1 = pull sample detail into the render
  include_results              INTEGER NOT NULL
  include_exceptions           INTEGER NOT NULL
  UNIQUE (working_paper_id, test_id)
  INDEX (test_id)

ReviewNote
  id                           TEXT PK
  working_paper_id             TEXT FK → WorkingPaper.id
  anchor                       TEXT                           -- "section:<id>" | "sample:<id>" | "result:<id>" | "."
  content_blob_id              TEXT FK → EncryptedBlob.id
  raised_by                    TEXT FK → User.id
  raised_at                    INTEGER NOT NULL
  status                       TEXT NOT NULL                  -- open | addressed | cleared | retained_for_history
  addressed_by                 TEXT FK → User.id
  addressed_at                 INTEGER
  cleared_by                   TEXT FK → User.id              -- reviewer who ticks the note off
  cleared_at                   INTEGER
  INDEX (working_paper_id, status)

SignOff
  id                           TEXT PK
  working_paper_id             TEXT FK → WorkingPaper.id
  user_id                      TEXT FK → User.id
  role_at_signoff              TEXT NOT NULL                  -- preparer | reviewer | partner
  action                       TEXT NOT NULL                  -- prepared | reviewed | partner_approved | reopened | locked
  signed_at                    INTEGER NOT NULL
  signature_blob_id            TEXT FK → EncryptedBlob.id     -- optional: captured signature image or PKI signature
  notes                        TEXT
  INDEX (working_paper_id, signed_at)
```

**Notes:**
- `WorkingPaper.wp_type` is advisory for the UI, not a DB rule. A `per_test` WP *usually* has exactly one `WPTestLink`, but the schema does not enforce that — firms mix conventions and migrations should preserve existing structure.
- `WPSection` is optional. Simple WPs use `WorkingPaper.content_blob_id` as a single body; structured WPs split into ordered sections, each with its own blob.
- `ReviewNote.anchor` is a lightweight locator string the UI resolves to a specific point in the rendered WP. `retained_for_history` keeps the thread visible post-close even though it has been addressed — the audit trail is more valuable than the tidiness of a cleared screen.
- `SignOff` is the immutable ledger of prepare → review → partner-approve → lock. Never deleted. A "reopen" creates a new `SignOff` row with `action=reopened`; the chain shows the full history.

---

## Cross-cutting: ActivityLog

Distinct from `ChangeLog`. `ChangeLog` is for sync replay and conflict detection (row-level, machine-oriented). `ActivityLog` is the audit-trail-for-humans — reviewer-facing, groupable by action, free-text annotations.

```
ActivityLog
  id                  TEXT PK
  engagement_id       TEXT FK → Engagement.id
  entity_type         TEXT NOT NULL
  entity_id           TEXT NOT NULL
  action              TEXT NOT NULL        -- created | updated | deleted | attested | reviewed | signed_off | reopened | ...
  performed_by        TEXT FK → User.id
  performed_at        INTEGER NOT NULL
  summary             TEXT                 -- human-readable, one line
  detail_json         TEXT                 -- structured change data, if relevant
  INDEX (engagement_id, performed_at)
  INDEX (entity_type, entity_id, performed_at)
```

Append-only. No UPDATE or DELETE path exposed in the application layer.

---

## Open data-model questions

- **JSON validation**: SQLite has `json_valid()`; decide between a CHECK constraint on every `_json` column, or validation at the application boundary. Currently validated at the application boundary (serde round-trip); revisit if corruption becomes a real risk.
- **Full-text search**: FTS5 virtual tables are the natural home for searching `Finding` (condition/criteria/cause/effect), `WorkingPaper.content`, `WPSection.body`, and `Evidence.description`. Blob content would need to be mirrored into the FTS table in plaintext — crosses the plaintext-at-rest line. Deferred until search demand surfaces; likely resolved by indexing an in-memory decrypt-on-query path instead.
- **Blob storage layout on disk**: directory structure per engagement, per blob type. Decide before first write so migrations don't bite. Proposed: `{app_data_dir}/blobs/<engagement_id>/<first_two_chars_of_blob_id>/<blob_id>.bin`. Two-char fan-out keeps any single directory under a few thousand files.
- **`ActivityLog` vs `ChangeLog` boundary**: reaffirmed — `ChangeLog` feeds sync/conflict and is row+field granular; `ActivityLog` is the human audit trail (coarser, with free-text summaries). When both would fire on the same mutation, both fire — overlap is the point.

Resolved during Modules 6-9 drafting (2026-04-24):
- **UUID format** — UUID v7 confirmed in use via `uuid` crate with `v7` feature.
- **FK enforcement** — `PRAGMA foreign_keys = ON` is set in `db::open_with_key`.
- **Library bundle format** — signed JSON + detached Ed25519 `.sig` file, shipped in-binary, verified on DB open. See `NOTES.md` and Module 5 notes.

These are captured here (not in `PROGRESS.md`) because they're schema-level, not project-level.
