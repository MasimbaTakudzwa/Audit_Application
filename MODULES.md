# Audit Application — Module Map

The app's architecture as thirteen functional modules plus cross-cutting concerns.

This file is the reference for "what modules exist, what entities they have, how they connect." For rationale behind specific decisions, see `NOTES.md`. For current build state and pending work, see `PROGRESS.md`.

## Overview

- Modules **1** (Identity & Licensing) and **12** (Sync & Storage) are foundations — everything else depends on them.
- Modules **2–9** are the core audit workflow.
- Module **10** is the free client-facing portal (server-only, separate auth realm).
- Module **11** (AI Layer) and Module **13** (Reporting) are infrastructure consumed by the workflow modules.

## The thirteen modules

### 1. Identity & Licensing
Who's logged in, what they can do, what their firm has paid for.

**Entities**: `Firm`, `User` (auditor side), `Role`, `License`, `SubscriptionPlan`, `PrepaidBalance`, `BYOKeyConfig`

### 2. Client Management
The organisations being audited. Distinct from client-portal users (Module 10).

**Entities**: `Client`, `ClientContact`, `Industry`, `ClientSettings`

### 3. Engagement Core
The audit itself — the container everything else lives inside.

**Entities**: `Engagement`, `EngagementPeriod`, `EngagementTeam`, `EngagementScope`, `EngagementBudget`, `EngagementStatus`

### 4. System Inventory
The client IT systems in scope for this engagement.

**Entities**: `System`, `SystemTemplate` (pre-built SAP / Oracle / core banking packs), `CustomSystem`

### 5. Risk & Control Library
Reusable methodology layer. Dev-shipped industry baseline + firm-level overrides (see Architectural Decisions).

**Entities**: `LibraryRisk`, `LibraryControl`, `TestProcedure` (test templates), `FrameworkMapping` (COBIT 2019 / NIST CSF / ISO 27001 / PCI), `ExpectedEvidenceChecklist`, `FirmOverride`

### 6. Fieldwork & Testing
Where rules, classical ML, and LLMs all show up.

**Entities**: `EngagementRisk` (cloned from library with `derived_from`), `EngagementControl` (cloned from library with `derived_from`), `Test` (instance of TestProcedure in this engagement), `Sample`, `SamplingPlan`, `TestResult`, `TestConclusion`, `Connector`, `DataImport`

### 7. Evidence Management
Everything that backs up a test.

**Entities**: `Evidence`, `EvidenceTag`, `EvidenceProvenance`, `PBCRequest`, `PBCStatus`, `PriorYearEvidenceLink` (for "unchanged" attestations)

### 8. Findings & Remediation
Control deficiencies and the client's response.

**Entities**: `Finding`, `FindingSeverity`, `RootCauseTaxonomy`, `ManagementActionPlan`, `FollowUp`, `RecurringFindingLink`

### 9. Working Papers & Review
Structured narrative documents and the review workflow. Supports per-test, per-section, or custom grouping (see Architectural Decisions).

**Entities**: `WorkingPaper`, `WPSection`, `WPTestLink` (many-to-many between WPs and Tests), `ReviewNote`, `ReviewStatus` (Draft / Prepared / In Review / Reviewed / Locked), `SignOff`

### 10. Client Portal
The free browser layer for clients. Server-only; separate auth realm from auditor users.

**Entities**: `PortalUser`, `PortalSession`, `DocumentUpload`, `Attestation`, `AttestationTemplate`, `ClientMessage`

### 11. AI Layer
Abstraction over whichever LLM provider is in use.

**Entities**: `ProviderAdapter` (Claude / OpenAI / Gemini / Meta / Ollama), `PromptTemplate`, `LLMUsageLog`, `PromptCache`, `RedactionRule`

### 12. Sync & Storage
Local-first with encrypted sync. Every mutable entity has a sync lifecycle.

**Entities**: `SyncRecord`, `ChangeLog`, `ConflictResolution`, `EncryptedBlob`, `KeychainEntry`

### 13. Reporting & Export
Mostly orchestration over other modules.

**Entities**: `Report`, `ReportTemplate`, `ExportJob`

## Cross-cutting concerns

Present across every module, not a module unto themselves:

- **Activity log / audit trail** — every mutation recorded: who, when, what changed. Append-only. See Architectural Decisions for retention policy.
- **Time tracking** — optional per firm; feeds engagement budgets and next-year scoping.
- **Search & tagging** — global search across entities.
- **Notifications** — in-app and email; respect per-user quiet hours.

## Load-bearing relationships

```
Firm ─┬─ User
      └─ Client ─── Engagement ─┬─ EngagementTeam (Users)
                                ├─ System
                                ├─ EngagementRisk ←→ EngagementControl ←→ TestProcedure
                                ├─ Test ─┬─ Sample
                                │        ├─ Evidence (many-to-many via TestEvidenceLink)
                                │        ├─ WorkingPaper (many-to-many via WPTestLink)
                                │        └─ Finding ─┬─ ManagementActionPlan
                                │                    └─ RecurringFindingLink → prior Finding
                                ├─ PBCRequest → Evidence
                                ├─ Connector
                                └─ PortalUser ─┬─ DocumentUpload → Evidence
                                               └─ Attestation

Library ─┬─ LibraryRisk → EngagementRisk (via derived_from)
         ├─ LibraryControl → EngagementControl (via derived_from)
         └─ TestProcedure (library) → TestProcedure (firm override) → Test instance
```

Every entity has a `SyncRecord`. Every LLM call has an `LLMUsageLog`. Every mutation has an `ActivityLog` entry.

## Where data lives

- **Local + encrypted-sync'd**: Modules 2–9 (all client engagement data). SQLCipher DB for structured data, AES-256-GCM for file blobs.
- **Server-only**: Module 10 (client portal — clients reach it via browser), proprietary crown jewels (AI prompts, library updates, cross-firm intelligence when it exists), licensing / billing state.
- **Local-only**: OS keychain entries, active session, queued sync changes before upload.

Principle: **the server never sees plaintext of engagement data**. It stores encrypted blobs and routes them between auditor devices and the client portal.

## Build order

**Foundations** (must come first):
- Module 12 (Sync & Storage)
- Module 1 (Identity & Licensing)

**Core domain** (next):
- Module 2 (Clients) → Module 3 (Engagements) → Module 4 (Systems) → Module 5 (Library)

**Workflow** (where most value surfaces):
- Module 6 (Testing) → Module 7 (Evidence) → Module 8 (Findings) → Module 9 (WPs)

**Deliverable + parallel tracks**:
- Module 13 (Reporting) — last; depends on 8 + 9
- Module 10 (Client Portal) — parallel, once 3 and 7 are stable
- Module 11 (AI Layer) — parallel, consumed by 6, 8, 9 but not blocking

## Architectural decisions

### Working paper granularity — three formats supported

The app supports three WP formats. An engagement can pick one or mix across sections:

- **Per-test WP** — one narrative document per `Test`. Most granular.
- **Per-section WP** — one document covering all tests in a section (e.g. "Change Management" with ten tests grouped under it).
- **Custom** — user-defined grouping, e.g. one WP covers three specific tests across sections.

Implementation: `WorkingPaper` is independent of `Test`. The `WPTestLink` join table allows zero, one, or many tests per WP. No one-to-one enforcement.

### Library ownership — dev-shipped baseline with firm-level overrides

- Risk and control library is curated and shipped by the product (industry standards: COBIT 2019, NIST CSF, ISO 27001, PCI, and additions over time).
- Firms can override, extend, or disable library entries for their own methodology.
- Library updates arrive via the existing licence-check mechanism; firms see "new library version available" and choose when to apply.
- Firm-level overrides live in a separate `FirmOverride` layer that survives library updates automatically.

### Engagement carry-forward — hybrid clone with `derived_from`

When a new engagement starts for a client, the app clones the prior engagement's `EngagementRisk`, `EngagementControl`, and `TestProcedure` records into the new engagement. Each cloned record carries:

- `derived_from` — pointer to the source record (prior engagement's equivalent, or library entry if starting fresh)
- `library_version` — which library revision the source was based on

This is **snapshot with lineage** — not pure linking, not pure cloning:

- Each engagement is a self-contained snapshot (audit defensibility preserved)
- Library updates don't retroactively change historical working papers
- Year-over-year recurring finding detection works via the `derived_from` chain
- Methodology tweaks for one engagement don't ripple to others
- Auditors can explicitly pull library updates into a live engagement when they choose

See `NOTES.md` for the full rationale and alternatives considered.

### Control ↔ System mapping — defaults plus manual override

- `LibraryControl` declares which system types it applies to via metadata (e.g., "privileged access review" → AD, SAP, SQL).
- When an engagement is scoped, default control-system mappings auto-populate based on systems in scope.
- Auditor can manually add, remove, or adjust mappings per engagement via the scoping UI.
- Overrides are local to the engagement and do not affect the library or other engagements.

### Evidence re-use year-to-year — fresh upload preferred, "unchanged" link available

- Default: fresh upload required each engagement (supports audit independence).
- Prior-year evidence is shown inline as reference when uploading.
- Auditor can mark an evidence item as "unchanged from prior year" — the prior-year `Evidence` record is linked as the current year's evidence via `PriorYearEvidenceLink`. The attestation of "unchanged" is recorded in the `ActivityLog`.
- The UI makes "unchanged" slightly less prominent than fresh upload — fresh upload remains the encouraged default.

### Activity log retention — forever within engagement, archived on close

- Every mutation (by whom, when, field-level old-vs-new value) is recorded in an append-only `ActivityLog` tied to the entity.
- Within an active engagement, all history is queryable directly from the entity detail view.
- On engagement close, the activity log is archived alongside the engagement as an immutable bundle — encrypted, retained indefinitely, and retrievable for regulator requests, peer review, or internal QA.
- Archive format: self-describing JSON inside an encrypted tarball.

## Open / deferred items at module level

None as of 2026-04-21. Detailed data-model schema per module is the next design step.
