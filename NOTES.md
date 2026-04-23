# Audit Application — design notes

Longer-form rationale for decisions that don't fit in `CLAUDE.md`. For current status, see `PROGRESS.md`.

## Why local-first over SaaS

The dominant IT audit tools are cloud-first. That works fine in well-connected environments but breaks in two ways for the target market:

1. **Bandwidth** — African client sites often have slow, unreliable internet. Auditors can't wait thirty seconds per page load, and they can't be blocked from working because a VPN dropped.
2. **Data residency** — some clients (especially in banking and government) require that their data not leave the engagement physically. A cloud-first tool makes this impossible and disqualifies the product in those sectors.

A desktop app with encrypted local storage and opt-in sync addresses both. The auditor carries the engagement on their laptop and syncs when convenient. The client portal, which does need internet, is the one externally-facing surface and runs on the client's side of the relationship — their connectivity, their problem.

## Why Tauri over Electron

- Smaller installer (10MB vs 150MB). Matters on low-bandwidth networks.
- Compiled Rust is meaningfully harder to reverse than a Node.js bundle. Relevant for anti-piracy.
- Lower RAM footprint. Matters on mid-range corporate laptops.
- Native OS integration without bridging through a Node runtime.
- **Trade-off**: smaller ecosystem, less documentation, more Rust to learn.

Electron remains a viable fallback if Rust becomes a blocker during development. The decision is tentative and will be committed only after the first prototype is attempted.

## Why "rule-based first"

Most "automation" in audit doesn't need AI. User access review is a JOIN. Sampling is a textbook formula. Duplicate detection is a GROUP BY. Anomaly detection is IsolationForest.

LLMs are:

- **Non-deterministic** — bad for auditable work where a reviewer has to reproduce the result
- **Expensive** — eats margin, harder to price competitively
- **A data-leak risk** if mishandled — eats client trust, which is the hardest asset to regain

Anywhere a deterministic solution exists, it's the better answer. LLMs are reserved for tasks that genuinely need judgement: drafting prose, extracting controls from unstructured policy PDFs, summarising interview notes, pattern-matching across free-form findings.

Calling something "AI-powered" when it's actually a ten-line SQL query is a marketing trap that raises user expectations and technical cost without improving the outcome.

## Why three pricing tiers

- **Subscription** maximises lifetime value and retention for committed users. Predictable revenue, predictable cost.
- **Prepaid** captures occasional users and those unwilling to commit to a subscription. Higher margin compensates for shorter commitment. The per-unit price sloping up for small packs is not greed — it's honest pricing against payment processor fees, which take a flat component per transaction.
- **BYO-key** captures enterprise firms that already have LLM contracts or strict data-handling requirements and wouldn't use the other tiers at all. The value-add is the *product*, not the AI — and at that level the customer is buying the audit workflow, structure, and IP.

Each tier extends reach into a segment the others wouldn't capture. The overlap between segments is small.

## Why multi-provider from day one

Reasons, in rough order of importance:

- **Vendor lock-in risk**: depending on a single provider means a pricing change or policy change by them directly hits the product, with little leverage to push back.
- **Enterprise compliance**: some firms are committed to a specific LLM provider for data-handling reasons (Azure OpenAI for certain compliance regimes, Google for regulated sectors, etc.). Locking in a single provider disqualifies the product in those sectors.
- **Price competition**: the cheapest model for a given task changes month to month. Being able to route a task to whichever provider is cheapest at the moment is a margin win.
- **Fallback reliability**: if one provider is rate-limited or degraded, the app continues operating by routing to another.

Building the provider-adapter interface up-front is relatively cheap. Retrofitting it later is expensive — the whole prompt layer has to be rewritten.

## Why per-client recurring findings before cross-client

**Per-client** (year-over-year for the same client):

- Clearly valuable to every customer from their first renewal engagement onward
- No privacy or contractual concerns — you already hold last year's working papers for this client
- Simple implementation — just query prior engagements for the same client_id

**Cross-client** (aggregated across many audits):

- Only valuable once many engagements exist in the system
- Requires careful anonymisation and opt-in
- Raises data-handling and contractual questions: does the firm's engagement letter with each client allow aggregated analysis? In most jurisdictions the default is "no" without explicit consent.

Ship per-client first. Revisit cross-client once a single firm has enough engagements for it to produce useful signal, and once the legal framework (engagement letter templates, consent language) is sorted.

## Why the client portal is free

Three reasons:

1. **Adoption**: clients will refuse to install or pay for a tool just to share files with their auditor. Free and browser-based is the only path that reaches them.
2. **Lock-in for the auditor product**: once a firm's clients are uploading evidence through the platform, switching to a competing audit tool means migrating every client too. That's friction the competitor has to overcome.
3. **Differentiation**: this is the main reason clients will prefer this platform over a vanilla audit tool bolted to SharePoint. It's also the cheapest part of the product to build (web app, S3-class storage, authentication) so "free" doesn't hurt margin much.

The product's revenue comes from the auditor side. The client side is a loss leader that exists to pull auditors in.

## Why no emoji / no decorative icons / no marketing adjectives

- **Professional signal**: auditors work on behalf of boards and regulators. Serious tools look serious.
- **International audience**: emoji render inconsistently across platforms and cultures. What looks friendly in one context reads as unprofessional or confusing in another.
- **Accessibility**: icons without labels fail screen readers. Labels without icons always work.
- **Taste**: Simba's writing site already follows the same restraint — the audit app should feel like it came from the same hand.

## Why hybrid-clone for engagement carry-forward

When a client is audited year after year, the new engagement needs to relate to the prior engagement's methodology (risks, controls, test procedures). Three approaches were considered:

**Pure linking (shared records across years).** The new engagement points to the *same* underlying `Control` record the prior engagement used — one record, many engagements.

- Pros: smaller database, automatic methodology consistency, trivial recurring-finding lookup (same primary key across years).
- Fatal con: audit defensibility. If the control description is updated mid-year to reflect new regulatory guidance, the prior year's working papers now reference text that didn't exist when the audit was signed off. A regulator or peer reviewer can't reconstruct what was actually tested.
- Mitigation would require a `Control` + `ControlVersion` scheme, which is functionally the same as cloning with lineage — so we may as well do the clone version directly.

**Pure cloning (copy with no backward reference).** The new engagement starts by copying all methodology into fresh records. No link to origin.

- Pros: each engagement is fully isolated; no ripple effects.
- Con: recurring-finding detection falls back to text matching across engagements — less reliable. No clean way to pull library updates into an in-flight engagement.

**Hybrid clone with `derived_from`** (the chosen approach). Clone into fresh records, but each clone carries:
- `derived_from` — pointer to the source record (prior engagement's equivalent, or library entry)
- `library_version` — which library revision the source was based on (enables later change detection)

Pros:
- Each engagement is a snapshot (audit defensibility preserved)
- Year-over-year lineage is explicit (reliable recurring detection via the `derived_from` chain)
- Library updates are opt-in per engagement
- Methodology tweaks for one engagement don't ripple to others

Con: slightly more complex than pure cloning (one extra column on cloned records, plus discipline to always set it).

The con is low cost and the benefits are large. This is also the pattern most mature audit tools converge on — useful sanity check.

## Library bundle format

The risk/control/test-procedure library ships as a signed JSON bundle, loaded into the DB at app startup. Two files per version:

- `app/src-tauri/resources/library/v<X.Y.Z>.json` — payload (risks, controls, test procedures, evidence checklists, framework mappings, plus a small header).
- `app/src-tauri/resources/library/v<X.Y.Z>.json.sig` — Ed25519 detached signature over the raw bytes of the JSON file. Hex-encoded ASCII, no newline.

**Why detached signature over canonical JSON.** Canonical JSON (sorted keys, stripped whitespace, normalised numbers) is annoying to get right across two implementations (Rust loader + tool). Signing the raw file bytes avoids the problem: whatever bytes shipped are what was signed. The bundle file is never re-serialised in flight — it's read, verified, then parsed.

**Why a signature at all.** Library bundles contain audit methodology. A swapped or tampered bundle changes what the app tests, which controls it references, and what framework mappings appear in reports. The signature protects against a malicious or accidental bundle swap at install or update time. It is not a licensing mechanism.

**Public key location.** Baked into the Rust binary as a `const [u8; 32]` in `src-tauri/src/library/verify.rs`. Swapping it requires a recompile. Intentionally: a product update that includes a new library version also includes a new verifier if the key rotates, so the key lifecycle is tied to release cadence.

**Private key location.** Outside the repo, outside the app. Held by Simba. The dev-machine convention is `~/.config/audit-app/signing/library.key` (0600 perms, user-only). Production signing happens on a machine Simba controls; the key is never checked in and is backed up to encrypted offline storage.

**Signing tool.** `tools/sign-library-bundle/` — a standalone Rust CLI, not part of the app binary. Subcommands: `keygen` (one-off at setup), `sign`, `verify`. The app never signs; it only verifies.

**Bundle shape** — one top-level object:

```json
{
  "version": "0.1.0",
  "published_at": 1745366400,
  "frameworks_included": ["COBIT 2019", "NIST CSF"],
  "risks": [
    {
      "code": "UAM-R-001",
      "title": "...",
      "description": "...",
      "applicable_system_types": ["generic-erp", "core-banking"],
      "default_inherent_rating": "high",
      "framework_mappings": [{"framework": "COBIT 2019", "reference": "DSS05.04"}]
    }
  ],
  "controls": [
    {
      "code": "UAM-C-001",
      "title": "...",
      "description": "...",
      "objective": "...",
      "applicable_system_types": ["generic-erp"],
      "control_type": "detective",
      "frequency": "quarterly",
      "related_risk_codes": ["UAM-R-001"],
      "framework_mappings": [{"framework": "NIST CSF", "reference": "PR.AC-1"}]
    }
  ],
  "test_procedures": [
    {
      "code": "UAM-T-001",
      "control_code": "UAM-C-001",
      "name": "...",
      "objective": "...",
      "steps": ["...", "..."],
      "sampling_default": "attribute",
      "automation_hint": "rule-based",
      "evidence_checklist": {"items": ["...", "..."]}
    }
  ]
}
```

**Why inline evidence checklists and framework mappings**, even though they are separate tables in the schema: authors should not have to invent cross-reference IDs. The loader splits inline structures into separate rows at insert time, generating UUIDs and resolving code-to-UUID links. The on-disk schema stays normalised; the author-facing format stays flat.

**Code-based cross-references throughout the bundle.** `control_code`, `related_risk_codes`, `test_procedure_code` — not UUIDs. The bundle never contains DB-layer UUIDs. This makes bundles reproducible across machines and avoids a class of "why is the same bundle generating different IDs" bugs.

**Version uniqueness.** `(code, library_version)` is unique per table. Re-loading the same version is idempotent: the loader checks for existing rows with the bundle's `library_version` and exits early if already installed. Upgrading to a new version inserts fresh rows and sets `superseded_by` on any prior-version rows that share a `code`.

**No DB-layer row IDs in the bundle.** Bundle entities have a `code` (stable, human-readable, e.g. `UAM-C-001`). The loader generates UUIDs on first insert. Cross-references inside the bundle use codes (`related_risk_codes`, `control_code`) not UUIDs, so the bundle author never has to invent IDs and the same bundle produces the same DB structure on any machine.

**What is not in the bundle.** No `FirmOverride` rows — those are firm-local customisations. No `Engagement`-level clones — those are written per-engagement at scope time. The bundle is the vendor-shipped baseline; everything downstream layers on top.

## Open question: how much of the client portal should reuse the writing-site codebase?

The writing site (`/Users/simsbgang/Desktop/Fun Website Project`) has an established visual language, typography, theme-toggle, and responsive behaviour. A case could be made to reuse some of its CSS and component patterns for the client portal to save time and maintain aesthetic coherence.

**Against reuse**: the writing site is a personal brand site and may evolve independently; coupling the audit portal's look to Simba's personal site could create awkward refactoring pressure later.

**For reuse**: same designer, same taste, same typographic system. No need to reinvent it.

Tentative answer: borrow the *aesthetic principles* (typography, palette, spacing, voice) but build the portal's CSS independently so the two can evolve separately. Revisit if duplication becomes painful.
