<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type DataImportSummary,
    type EngagementOverview,
    type EngagementSummary,
    type EvidenceSummary,
    type FindingSummary,
    type LibraryControlSummary,
    type SeveritySummary,
    type TestResultSummary,
    type TestSummary,
  } from "../api/tauri";
  import FindingEditor from "../components/FindingEditor.svelte";
  import {
    currentEngagementId,
    currentRoute,
    openWorkingPaper,
  } from "../stores/router";

  type PurposeTag =
    | "ad_export"
    | "entra_export"
    | "hr_active"
    | "hr_leavers"
    | "hr_master"
    | "payroll"
    | "badge_log"
    | "change_log"
    | "backup_log"
    | "transaction_register"
    | "deploy_permissions"
    | "source_access"
    | "access_review"
    | "other";

  const PURPOSE_OPTIONS: { id: PurposeTag; label: string; hint: string }[] = [
    { id: "ad_export",   label: "AD export",       hint: "Active Directory user dump (enabled + disabled)" },
    { id: "entra_export",label: "Entra export",    hint: "Azure AD / Entra user dump" },
    { id: "hr_active",   label: "HR active list",  hint: "Employees currently on payroll" },
    { id: "hr_leavers",  label: "HR leavers list", hint: "Terminated or departed employees" },
    { id: "hr_master",   label: "HR master roster", hint: "Authoritative list of current employees (for orphan-account checks)" },
    { id: "payroll",     label: "Payroll run",     hint: "Payroll register for the period" },
    { id: "badge_log",   label: "Badge / access log", hint: "Physical access or entry log" },
    { id: "change_log",  label: "Change log",      hint: "Change-management export (ServiceNow, Jira, Remedy, …)" },
    { id: "backup_log",  label: "Backup log",      hint: "Backup-tool job history (Veeam, Commvault, Rubrik, …)" },
    { id: "transaction_register", label: "Transaction register", hint: "Export of the in-scope transaction population, with an amount column" },
    { id: "deploy_permissions", label: "Deploy permissions", hint: "Production deployment tool's role or permission matrix (for dev-vs-deploy SoD)" },
    { id: "source_access", label: "Source repository access", hint: "Source host or change-authoring tool's access export (for dev-vs-deploy SoD)" },
    { id: "access_review", label: "Access-review log", hint: "Periodic recertification sign-off: one row per reviewed user, optionally with decision, review_date, and remediation_status columns" },
    { id: "other",       label: "Other",           hint: "Any other evidence you want to retain" },
  ];

  // Test codes for which the backend has a dispatchable matcher today. Keep
  // this list in sync with `MatcherRule::for_test_code` in Rust — any code
  // in here must also be dispatchable there, or the button will error when
  // pressed.
  const MATCHER_ENABLED_CODES = new Set([
    "UAM-T-001",
    "UAM-T-002",
    "UAM-T-003",
    "UAM-T-004",
    "CHG-T-001",
    "CHG-T-002",
    "BKP-T-001",
    "ITAC-T-001",
    "ITAC-T-002",
    "ITAC-T-003",
    "ITAC-T-004",
  ]);

  let loading = $state(true);
  let err = $state("");

  let engagement = $state<EngagementSummary | null>(null);
  let overview = $state<EngagementOverview | null>(null);
  let imports = $state<DataImportSummary[]>([]);
  let tests = $state<TestSummary[]>([]);
  let results = $state<TestResultSummary[]>([]);
  let findings = $state<FindingSummary[]>([]);
  let severities = $state<SeveritySummary[]>([]);
  let evidence = $state<EvidenceSummary[]>([]);

  // Evidence upload form state.
  let showEvidenceUpload = $state(false);
  let submittingEvidence = $state(false);
  let evidenceUploadErr = $state("");
  let evTitle = $state("");
  let evDescription = $state("");
  let evObtainedFrom = $state("");
  let evTestId = $state<string>("");
  let evFile = $state<File | null>(null);
  let evFileInput: HTMLInputElement | null = $state(null);

  // Per-row download state to disable the button while bytes are fetched.
  let downloadingEvidenceId = $state<string | null>(null);
  let downloadErr = $state("");

  // Elevation state — keyed by test_result id to avoid blocking the whole table.
  let elevatingResultId = $state<string | null>(null);
  let elevateErr = $state("");

  // Finding editor state — which finding row is expanded into the editor.
  let editingFindingId = $state<string | null>(null);

  // Upload state.
  let showUpload = $state(false);
  let submittingUpload = $state(false);
  let uploadErr = $state("");
  let purpose = $state<PurposeTag>("ad_export");
  let selectedFile = $state<File | null>(null);
  let fileInput: HTMLInputElement | null = $state(null);

  // Library control picker state.
  let showLibraryPicker = $state(false);
  let libraryLoading = $state(false);
  let libraryErr = $state("");
  let libraryControls = $state<LibraryControlSummary[]>([]);
  let selectedLibraryControl = $state<string>("");
  let addingControl = $state(false);
  let addControlErr = $state("");

  // Matcher run state — keyed by test id so clicking one doesn't spin the row next to it.
  let runningTestId = $state<string | null>(null);
  let runErr = $state("");

  const engagementIdValue = $derived($currentEngagementId);

  onMount(async () => {
    if (!engagementIdValue) {
      currentRoute.set("engagements");
      return;
    }
    await load(engagementIdValue);
  });

  async function load(id: string) {
    loading = true;
    err = "";
    try {
      const [
        all,
        ov,
        dataImports,
        testList,
        resultList,
        findingList,
        severityList,
        evidenceList,
      ] = await Promise.all([
        api.listEngagements(),
        api.engagementOverview(id),
        api.engagementListDataImports(id),
        api.engagementListTests(id),
        api.engagementListTestResults(id),
        api.engagementListFindings(id),
        api.listFindingSeverities(),
        api.engagementListEvidence(id),
      ]);
      const match = all.find((e) => e.id === id);
      if (!match) {
        err = "Engagement not found. It may have been removed.";
        engagement = null;
      } else {
        engagement = match;
      }
      overview = ov;
      imports = dataImports;
      tests = testList;
      results = resultList;
      findings = findingList;
      severities = severityList;
      evidence = evidenceList;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  }

  function back() {
    currentEngagementId.set(null);
    currentRoute.set("engagements");
  }

  // Narrow refresh — re-pull just the overview synthesis after a mutation.
  // Cheaper than calling load() (which would re-fetch every table on the
  // page) and keeps the counters / risk strip / activity timeline in sync
  // with what the auditor has just done.
  async function refreshOverview() {
    if (!engagement) return;
    try {
      overview = await api.engagementOverview(engagement.id);
    } catch (e) {
      // Leave the prior overview visible on failure rather than blanking
      // it out — the user has the rest of the page; an Overview-refresh
      // hiccup shouldn't blow up the surface.
      console.warn("overview refresh failed", e);
    }
  }

  function openUpload() {
    showUpload = true;
    uploadErr = "";
    purpose = "ad_export";
    selectedFile = null;
    if (fileInput) fileInput.value = "";
  }

  function cancelUpload() {
    showUpload = false;
    uploadErr = "";
    selectedFile = null;
    if (fileInput) fileInput.value = "";
  }

  function onFileChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    selectedFile = input.files && input.files.length > 0 ? input.files[0] : null;
  }

  function extensionOf(name: string): string {
    const dot = name.lastIndexOf(".");
    return dot >= 0 ? name.slice(dot + 1).toLowerCase() : "";
  }

  async function submitUpload(event: Event) {
    event.preventDefault();
    if (!engagement || !selectedFile) return;
    uploadErr = "";
    submittingUpload = true;
    try {
      const buf = await selectedFile.arrayBuffer();
      const content = Array.from(new Uint8Array(buf));
      const sourceKind = extensionOf(selectedFile.name) || "binary";
      const row = await api.engagementUploadDataImport({
        engagement_id: engagement.id,
        system_id: null,
        source_kind: sourceKind,
        purpose_tag: purpose,
        filename: selectedFile.name,
        mime_type: selectedFile.type || null,
        content,
      });
      imports = [row, ...imports];
      evidence = await api.engagementListEvidence(engagement.id);
      showUpload = false;
      selectedFile = null;
      if (fileInput) fileInput.value = "";
      await refreshOverview();
    } catch (e) {
      uploadErr = String(e);
    } finally {
      submittingUpload = false;
    }
  }

  async function openLibraryPicker() {
    showLibraryPicker = true;
    addControlErr = "";
    if (libraryControls.length === 0 && !libraryLoading) {
      libraryLoading = true;
      libraryErr = "";
      try {
        libraryControls = await api.libraryListControls();
        selectedLibraryControl =
          libraryControls.length > 0 ? libraryControls[0].id : "";
      } catch (e) {
        libraryErr = String(e);
      } finally {
        libraryLoading = false;
      }
    }
  }

  function closeLibraryPicker() {
    showLibraryPicker = false;
    addControlErr = "";
  }

  async function addControl(event: Event) {
    event.preventDefault();
    if (!engagement || !selectedLibraryControl) return;
    addControlErr = "";
    addingControl = true;
    try {
      await api.engagementAddLibraryControl({
        engagement_id: engagement.id,
        library_control_id: selectedLibraryControl,
        system_id: null,
      });
      tests = await api.engagementListTests(engagement.id);
      showLibraryPicker = false;
      await refreshOverview();
    } catch (e) {
      addControlErr = String(e);
    } finally {
      addingControl = false;
    }
  }

  function onFindingSaved(updated: FindingSummary) {
    findings = findings.map((f) => (f.id === updated.id ? updated : f));
    editingFindingId = null;
    void refreshOverview();
  }

  async function elevateFinding(result: TestResultSummary) {
    if (!engagement || elevatingResultId) return;
    elevatingResultId = result.id;
    elevateErr = "";
    try {
      await api.engagementElevateFinding({
        test_result_id: result.id,
        title: null,
        severity_id: null,
      });
      const [refreshedResults, refreshedFindings] = await Promise.all([
        api.engagementListTestResults(engagement.id),
        api.engagementListFindings(engagement.id),
      ]);
      results = refreshedResults;
      findings = refreshedFindings;
      await refreshOverview();
    } catch (e) {
      elevateErr = String(e);
    } finally {
      elevatingResultId = null;
    }
  }

  async function runMatcher(test: TestSummary) {
    if (!engagement || runningTestId) return;
    runningTestId = test.id;
    runErr = "";
    try {
      await api.engagementRunMatcher({
        test_id: test.id,
        overrides: null,
      });
      const [refreshedTests, refreshedResults, refreshedEvidence] = await Promise.all([
        api.engagementListTests(engagement.id),
        api.engagementListTestResults(engagement.id),
        api.engagementListEvidence(engagement.id),
      ]);
      tests = refreshedTests;
      results = refreshedResults;
      evidence = refreshedEvidence;
      await refreshOverview();
    } catch (e) {
      runErr = String(e);
    } finally {
      runningTestId = null;
    }
  }

  function openEvidenceUpload() {
    showEvidenceUpload = true;
    evidenceUploadErr = "";
    evTitle = "";
    evDescription = "";
    evObtainedFrom = "";
    evTestId = "";
    evFile = null;
    if (evFileInput) evFileInput.value = "";
  }

  function cancelEvidenceUpload() {
    showEvidenceUpload = false;
    evidenceUploadErr = "";
    evFile = null;
    if (evFileInput) evFileInput.value = "";
  }

  function onEvidenceFileChange(event: Event) {
    const input = event.currentTarget as HTMLInputElement;
    evFile = input.files && input.files.length > 0 ? input.files[0] : null;
  }

  async function submitEvidenceUpload(event: Event) {
    event.preventDefault();
    if (!engagement || !evFile) return;
    evidenceUploadErr = "";
    submittingEvidence = true;
    try {
      const buf = await evFile.arrayBuffer();
      const content = Array.from(new Uint8Array(buf));
      const row = await api.engagementUploadEvidence({
        engagement_id: engagement.id,
        title: evTitle,
        description: evDescription.trim() ? evDescription : null,
        obtained_from: evObtainedFrom.trim() ? evObtainedFrom : null,
        obtained_at: null,
        test_id: evTestId || null,
        finding_id: null,
        filename: evFile.name,
        mime_type: evFile.type || null,
        content,
      });
      evidence = [row, ...evidence];
      showEvidenceUpload = false;
      evFile = null;
      if (evFileInput) evFileInput.value = "";
      await refreshOverview();
    } catch (e) {
      evidenceUploadErr = String(e);
    } finally {
      submittingEvidence = false;
    }
  }

  async function downloadEvidence(row: EvidenceSummary) {
    if (downloadingEvidenceId) return;
    downloadingEvidenceId = row.id;
    downloadErr = "";
    try {
      const payload = await api.engagementDownloadEvidence(row.id);
      const bytes = new Uint8Array(payload.content);
      const blob = new Blob([bytes], {
        type: payload.mime_type ?? "application/octet-stream",
      });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = payload.filename ?? `${row.id}.bin`;
      document.body.appendChild(a);
      a.click();
      document.body.removeChild(a);
      URL.revokeObjectURL(url);
    } catch (e) {
      downloadErr = String(e);
    } finally {
      downloadingEvidenceId = null;
    }
  }

  function evidenceSourceLabel(source: string): string {
    switch (source) {
      case "auditor_upload":  return "Auditor upload";
      case "data_import":     return "Data import";
      case "matcher_report":  return "Matcher report";
      case "client_portal":   return "Client portal";
      case "prior_year_link": return "Prior year";
      default:                return source.replace(/_/g, " ");
    }
  }

  function fmtDate(ts: number) {
    return new Date(ts * 1000).toLocaleString("en-GB", {
      day: "numeric",
      month: "short",
      year: "numeric",
      hour: "2-digit",
      minute: "2-digit",
    });
  }

  function fmtPurpose(tag: string | null): string {
    if (!tag) return "—";
    const match = PURPOSE_OPTIONS.find((o) => o.id === tag);
    return match ? match.label : tag;
  }

  function fmtSize(bytes: number | null): string {
    if (bytes == null) return "—";
    if (bytes < 1024) return `${bytes} B`;
    if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} kB`;
    return `${(bytes / (1024 * 1024)).toFixed(2)} MB`;
  }

  function outcomeLabel(outcome: string): string {
    switch (outcome) {
      case "pass":      return "Passed";
      case "exception": return "Exceptions";
      case "fail":      return "Failed";
      default:          return outcome;
    }
  }

  function testStatusLabel(status: string): string {
    return status.replace(/_/g, " ");
  }

  function severityLabel(id: string | null): string {
    if (!id) return "—";
    const match = severities.find((s) => s.id === id);
    return match ? match.name : id;
  }

  function severityPillClass(id: string | null): string {
    return id ? `pill pill-${id}` : "pill";
  }

  // -------- Today / Overview helpers --------

  function coverageStateLabel(state: string): string {
    switch (state) {
      case "uncovered":
        return "No controls linked";
      case "untested":
        return "Controls in place, no test results yet";
      case "tested_clean":
        return "Tested, no exceptions";
      case "tested_with_exceptions":
        return "Tested, exceptions raised";
      default:
        return state;
    }
  }

  function ratingPillClass(rating: string): string {
    return `pill pill-rating-${rating.toLowerCase()}`;
  }

  function inflect(n: number, singular: string, plural?: string): string {
    return n === 1 ? singular : plural ?? `${singular}s`;
  }

  // Humanise an ActivityLog action token. Tokens come in as e.g.
  // "matcher_run", "created", "elevate_to_finding"; render them in
  // sentence case with spaces.
  function formatActivityAction(action: string): string {
    const normalised = action.replace(/_/g, " ").trim();
    if (!normalised) return action;
    return normalised.charAt(0).toUpperCase() + normalised.slice(1);
  }

  // Turn a Unix epoch (seconds) into "n minutes ago" / "yesterday" /
  // "3 days ago" — short form used in the activity timeline. Falls back
  // to the full date for anything older than a week.
  function relativeTime(secs: number): string {
    const now = Math.floor(Date.now() / 1000);
    const delta = now - secs;
    if (delta < 60) return "Just now";
    if (delta < 3600) {
      const m = Math.floor(delta / 60);
      return `${m} ${inflect(m, "minute")} ago`;
    }
    if (delta < 86400) {
      const h = Math.floor(delta / 3600);
      return `${h} ${inflect(h, "hour")} ago`;
    }
    if (delta < 86400 * 7) {
      const d = Math.floor(delta / 86400);
      return `${d} ${inflect(d, "day")} ago`;
    }
    return fmtDate(secs);
  }
</script>

<header>
  <button type="button" class="back" onclick={back}>← Engagements</button>
  {#if loading}
    <p class="faint">Loading…</p>
  {:else if err}
    <p class="faint">{err}</p>
  {:else if engagement}
    <span class="label">Engagement</span>
    <h1>{engagement.name}</h1>
    <p class="muted">
      {engagement.client_name} · {engagement.fiscal_year ?? "No period set"} ·
      <span class="accent">{engagement.status}</span>
    </p>
  {/if}
</header>

<hr />

{#if !loading && !err && engagement}
  {#if overview}
    <section class="block overview">
      <div class="block-head">
        <div>
          <h2>Today</h2>
          <p class="muted">
            {#if overview.engagement.period_start && overview.engagement.period_end}
              Period {overview.engagement.period_start} → {overview.engagement.period_end} ·
            {/if}
            {overview.engagement.client_name} ·
            <span class="accent">{overview.engagement.status}</span>
            {#if overview.engagement.lead_partner_name}
              · Led by {overview.engagement.lead_partner_name}
            {/if}
            · Library {overview.engagement.library_version_at_start}
          </p>
        </div>
      </div>

      <!-- Status counters -->
      <div class="overview-grid">
        <div class="overview-card">
          <span class="label">Risks</span>
          <p class="stat">{overview.status_counts.risks_total}</p>
          {#if overview.status_counts.risks_total === 0}
            <p class="faint">Add a library control to seed risks</p>
          {:else}
            {@const uncovered = overview.risk_coverage.filter(
              (r) => r.coverage_state === "uncovered",
            ).length}
            {@const untested = overview.risk_coverage.filter(
              (r) => r.coverage_state === "untested",
            ).length}
            {@const flagged = overview.risk_coverage.filter(
              (r) => r.coverage_state === "tested_with_exceptions",
            ).length}
            <p class="faint">
              {uncovered} uncovered · {untested} untested · {flagged} flagged
            </p>
          {/if}
        </div>

        <div class="overview-card">
          <span class="label">Tests</span>
          <p class="stat">{overview.status_counts.tests_total}</p>
          {#if overview.status_counts.tests_total === 0}
            <p class="faint">No procedures yet</p>
          {:else}
            <p class="faint">
              {overview.status_counts.tests_in_review}
              in review ·
              {overview.status_counts.tests_completed}
              completed ·
              {overview.status_counts.tests_not_started}
              not started
            </p>
          {/if}
        </div>

        <div class="overview-card">
          <span class="label">Findings</span>
          <p class="stat">{overview.status_counts.findings_total}</p>
          {#if overview.status_counts.findings_total === 0}
            <p class="faint">No findings raised</p>
          {:else}
            <p class="faint">
              {overview.status_counts.findings_critical +
                overview.status_counts.findings_high}
              critical/high ·
              {overview.status_counts.findings_draft}
              draft ·
              {overview.status_counts.findings_closed}
              closed
            </p>
          {/if}
        </div>

        <div class="overview-card">
          <span class="label">Evidence</span>
          <p class="stat">{overview.status_counts.evidence_total}</p>
          <p class="faint">
            {overview.status_counts.data_imports_total}
            {inflect(overview.status_counts.data_imports_total, "import")} ·
            {overview.status_counts.results_exception +
              overview.status_counts.results_fail}
            matcher
            {inflect(
              overview.status_counts.results_exception +
                overview.status_counts.results_fail,
              "exception",
            )}
          </p>
        </div>
      </div>

      <!-- Risk coverage strip -->
      {#if overview.risk_coverage.length > 0}
        <div class="overview-section">
          <h3>Risk coverage</h3>
          <div class="risk-strip">
            {#each overview.risk_coverage as risk (risk.risk_id)}
              <article class="risk-card cov-{risk.coverage_state}">
                <div class="risk-card-head">
                  <span class="label">{risk.risk_code}</span>
                  <span class={ratingPillClass(risk.inherent_rating)}
                    >{risk.inherent_rating}</span
                  >
                </div>
                <p class="risk-title">{risk.risk_title}</p>
                <p class="faint coverage-state">
                  {coverageStateLabel(risk.coverage_state)}
                </p>
                <p class="faint risk-counts">
                  {risk.control_count}
                  {inflect(risk.control_count, "ctrl", "ctrls")} ·
                  {risk.test_count}
                  {inflect(risk.test_count, "test")} ·
                  {risk.findings_open}
                  open
                </p>
              </article>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Two-column row: needs attention + recent activity -->
      <div class="two-col">
        <div class="overview-section">
          <h3>Needs attention</h3>
          {#if overview.needs_attention.length === 0}
            <p class="muted">Nothing pending. Worth a moment to review?</p>
          {:else}
            <ul class="attention-list">
              {#each overview.needs_attention as item, idx (idx)}
                <li class="attention-item priority-{item.priority}">
                  <span class="dot" aria-hidden="true"></span>
                  <span class="attention-label">{item.label}</span>
                </li>
              {/each}
            </ul>
          {/if}
        </div>

        <div class="overview-section">
          <h3>Recent activity</h3>
          {#if overview.recent_activity.length === 0}
            <p class="muted">No activity yet.</p>
          {:else}
            <ul class="activity-list">
              {#each overview.recent_activity as entry, idx (idx)}
                <li>
                  <span class="faint when">{relativeTime(entry.at)}</span>
                  <span class="what">
                    <strong>{entry.actor_name ?? "Someone"}</strong>
                    {formatActivityAction(entry.action)}
                    <span class="faint">
                      on {entry.entity_type}
                    </span>
                  </span>
                  {#if entry.summary}
                    <span class="faint summary-line">{entry.summary}</span>
                  {/if}
                </li>
              {/each}
            </ul>
          {/if}
        </div>
      </div>
    </section>
  {/if}

  <section class="block">
    <div class="block-head">
      <div>
        <h2>Data imports</h2>
        <p class="muted">
          Upload AD / Entra user dumps, HR active + leaver lists, payroll registers, and
          other evidence needed for testing. Files are encrypted on disk with this
          engagement's content key.
        </p>
      </div>
      {#if !showUpload}
        <button type="button" class="primary" onclick={openUpload}>Upload file</button>
      {/if}
    </div>

    {#if showUpload}
      <form class="card form" onsubmit={submitUpload}>
        <h3>Upload a file</h3>

        <label>
          <span class="label">Purpose</span>
          <select bind:value={purpose} required>
            {#each PURPOSE_OPTIONS as opt (opt.id)}
              <option value={opt.id}>{opt.label}</option>
            {/each}
          </select>
          <span class="faint hint">
            {PURPOSE_OPTIONS.find((o) => o.id === purpose)?.hint}
          </span>
        </label>

        <label>
          <span class="label">File</span>
          <input
            type="file"
            bind:this={fileInput}
            onchange={onFileChange}
            required
          />
          {#if selectedFile}
            <span class="faint hint">
              {selectedFile.name} · {fmtSize(selectedFile.size)}
            </span>
          {/if}
        </label>

        {#if uploadErr}
          <p class="form-err">{uploadErr}</p>
        {/if}

        <div class="form-actions">
          <button type="button" onclick={cancelUpload} disabled={submittingUpload}>
            Cancel
          </button>
          <button
            type="submit"
            class="primary"
            disabled={submittingUpload || !selectedFile}
          >
            {submittingUpload ? "Uploading…" : "Upload"}
          </button>
        </div>
      </form>
    {/if}

    {#if imports.length === 0 && !showUpload}
      <div class="card empty">
        <h3>No data imports yet</h3>
        <p class="muted">
          The access review expects at least an AD / Entra export and an HR leavers
          list. Upload the files the client provided to get started.
        </p>
      </div>
    {:else if imports.length > 0}
      <table>
        <thead>
          <tr>
            <th>File</th>
            <th>Purpose</th>
            <th>Rows</th>
            <th>Size</th>
            <th>Uploaded</th>
            <th>By</th>
          </tr>
        </thead>
        <tbody>
          {#each imports as imp (imp.id)}
            <tr>
              <td>{imp.filename ?? "—"}</td>
              <td class="muted">{fmtPurpose(imp.purpose_tag)}</td>
              <td class="muted">{imp.row_count ?? "—"}</td>
              <td class="muted">{fmtSize(imp.plaintext_size)}</td>
              <td class="faint">{fmtDate(imp.imported_at)}</td>
              <td class="faint">{imp.imported_by_name ?? "—"}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  </section>

  <section class="block">
    <div class="block-head">
      <div>
        <h2>Tests</h2>
        <p class="muted">
          Controls added from the library are cloned into this engagement with their
          test procedures. Rule-based tests can be run against uploaded data; others
          require manual fieldwork.
        </p>
      </div>
      {#if !showLibraryPicker}
        <button type="button" class="primary" onclick={openLibraryPicker}>
          Add library control
        </button>
      {/if}
    </div>

    {#if showLibraryPicker}
      <form class="card form" onsubmit={addControl}>
        <h3>Add a library control</h3>
        {#if libraryLoading}
          <p class="faint">Loading library…</p>
        {:else if libraryErr}
          <p class="form-err">{libraryErr}</p>
        {:else}
          <label>
            <span class="label">Control</span>
            <select bind:value={selectedLibraryControl} required>
              {#each libraryControls as c (c.id)}
                <option value={c.id}>{c.code} · {c.title}</option>
              {/each}
            </select>
          </label>
          {#if addControlErr}
            <p class="form-err">{addControlErr}</p>
          {/if}
          <div class="form-actions">
            <button type="button" onclick={closeLibraryPicker} disabled={addingControl}>
              Cancel
            </button>
            <button
              type="submit"
              class="primary"
              disabled={addingControl || !selectedLibraryControl}
            >
              {addingControl ? "Adding…" : "Add"}
            </button>
          </div>
        {/if}
      </form>
    {/if}

    {#if tests.length === 0 && !showLibraryPicker}
      <div class="card empty">
        <h3>No tests yet</h3>
        <p class="muted">
          Add a library control (for example <code>UAM-C-001</code>) to clone its test
          procedures into this engagement.
        </p>
      </div>
    {:else if tests.length > 0}
      <table>
        <thead>
          <tr>
            <th>Test</th>
            <th>Control</th>
            <th>Tier</th>
            <th>Status</th>
            <th>Last run</th>
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {#each tests as t (t.id)}
            <tr>
              <td>
                <div>{t.code}</div>
                <div class="faint small">{t.name}</div>
              </td>
              <td class="muted">{t.control_code}</td>
              <td class="muted">{t.automation_tier.replace(/_/g, " ")}</td>
              <td class="accent">{testStatusLabel(t.status)}</td>
              <td class="faint">
                {#if t.latest_result_at}
                  {outcomeLabel(t.latest_result_outcome ?? "")}
                  {#if t.latest_result_evidence_count && t.latest_result_evidence_count > 0}
                    · {t.latest_result_evidence_count} exception{t.latest_result_evidence_count === 1 ? "" : "s"}
                  {/if}
                  <div class="small">{fmtDate(t.latest_result_at)}</div>
                {:else}
                  —
                {/if}
              </td>
              <td class="actions-col">
                <div class="row-actions">
                  <button
                    type="button"
                    class="link"
                    onclick={() => engagement && openWorkingPaper(engagement.id, t.id)}
                  >
                    Open
                  </button>
                  {#if t.automation_tier === "rule_based" && MATCHER_ENABLED_CODES.has(t.code)}
                    <button
                      type="button"
                      class="link"
                      onclick={() => runMatcher(t)}
                      disabled={runningTestId !== null}
                    >
                      {runningTestId === t.id ? "Running…" : "Run matcher"}
                    </button>
                  {:else}
                    <span class="faint small">manual</span>
                  {/if}
                </div>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if runErr}
        <p class="form-err">{runErr}</p>
      {/if}
    {/if}
  </section>

  <section class="block">
    <div class="block-head">
      <div>
        <h2>Test results</h2>
        <p class="muted">
          Each run of a matcher produces a test result. Exceptions can be escalated to
          findings for the report.
        </p>
      </div>
    </div>

    {#if results.length === 0}
      <div class="card empty">
        <h3>No test results yet</h3>
        <p class="muted">
          Run a matcher from the Tests section once the relevant data imports are in
          place.
        </p>
      </div>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Test</th>
            <th>Outcome</th>
            <th>Exceptions</th>
            <th>Population</th>
            <th>Performed</th>
            <th>By</th>
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {#each results as r (r.id)}
            <tr>
              <td>
                <div>{r.test_code}</div>
                <div class="faint small">{r.exception_summary ?? ""}</div>
              </td>
              <td>
                <span class="pill pill-{r.outcome}">{outcomeLabel(r.outcome)}</span>
              </td>
              <td class="muted">{r.evidence_count}</td>
              <td class="faint small">{r.population_ref_label ?? "—"}</td>
              <td class="faint">{fmtDate(r.performed_at)}</td>
              <td class="faint">{r.performed_by_name ?? "—"}</td>
              <td class="actions-col">
                {#if r.outcome === "exception" && !r.has_linked_finding}
                  <button
                    type="button"
                    class="link"
                    onclick={() => elevateFinding(r)}
                    disabled={elevatingResultId !== null}
                  >
                    {elevatingResultId === r.id ? "Elevating…" : "Create finding"}
                  </button>
                {:else if r.has_linked_finding}
                  <span class="faint small">elevated</span>
                {:else}
                  <span class="faint small">—</span>
                {/if}
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if elevateErr}
        <p class="form-err">{elevateErr}</p>
      {/if}
    {/if}
  </section>

  <section class="block">
    <div class="block-head">
      <div>
        <h2>Findings</h2>
        <p class="muted">
          Draft findings raised from test exceptions. Condition and recommendation
          text start generic — refine them before the report goes to the client.
        </p>
      </div>
    </div>

    {#if findings.length === 0}
      <div class="card empty">
        <h3>No findings yet</h3>
        <p class="muted">
          Findings appear here when you elevate an exception from the Test results
          table above.
        </p>
      </div>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Code</th>
            <th>Title</th>
            <th>Severity</th>
            <th>Status</th>
            <th>Control</th>
            <th>Identified</th>
            <th>By</th>
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {#each findings as f (f.id)}
            {#if editingFindingId === f.id}
              <tr class="editing">
                <td colspan="8">
                  <div class="finding-form">
                    <FindingEditor
                      finding={f}
                      {severities}
                      onSaved={onFindingSaved}
                      onCancel={() => (editingFindingId = null)}
                    />
                  </div>
                </td>
              </tr>
            {:else}
              <tr>
                <td><code>{f.code}</code></td>
                <td>
                  <div>{f.title}</div>
                  {#if f.condition_text}
                    <div class="faint small">{f.condition_text}</div>
                  {/if}
                </td>
                <td>
                  <span class={severityPillClass(f.severity_id)}>
                    {severityLabel(f.severity_id)}
                  </span>
                </td>
                <td class="accent">{f.status.replace(/_/g, " ")}</td>
                <td class="muted">{f.control_code ?? "—"}</td>
                <td class="faint">{fmtDate(f.identified_at)}</td>
                <td class="faint">{f.identified_by_name ?? "—"}</td>
                <td class="actions-col">
                  <div class="row-actions">
                    {#if f.test_id && engagement}
                      <button
                        type="button"
                        class="link"
                        onclick={() =>
                          engagement &&
                          f.test_id &&
                          openWorkingPaper(engagement.id, f.test_id)}
                      >
                        Open
                      </button>
                    {/if}
                    <button
                      type="button"
                      class="link"
                      onclick={() => (editingFindingId = f.id)}
                      disabled={editingFindingId !== null}
                    >
                      Edit
                    </button>
                  </div>
                </td>
              </tr>
            {/if}
          {/each}
        </tbody>
      </table>
    {/if}
  </section>

  <section class="block">
    <div class="block-head">
      <div>
        <h2>Evidence</h2>
        <p class="muted">
          Every data import, matcher run, and auditor upload is tracked here with
          its source and chain of custody. Evidence is encrypted at rest and
          decrypted on demand when downloaded.
        </p>
      </div>
      {#if !showEvidenceUpload}
        <button type="button" class="primary" onclick={openEvidenceUpload}>
          Upload evidence
        </button>
      {/if}
    </div>

    {#if showEvidenceUpload}
      <form class="card form" onsubmit={submitEvidenceUpload}>
        <h3>Upload evidence</h3>
        <label>
          <span class="label">Title</span>
          <input type="text" bind:value={evTitle} required />
          <span class="faint hint">
            A short description so reviewers can find this in the list.
          </span>
        </label>
        <label>
          <span class="label">Obtained from</span>
          <input type="text" bind:value={evObtainedFrom} placeholder="e.g. IT manager email" />
        </label>
        <label>
          <span class="label">Related test</span>
          <select bind:value={evTestId}>
            <option value="">— Engagement-level (no specific test)</option>
            {#each tests as t (t.id)}
              <option value={t.id}>{t.code} · {t.name}</option>
            {/each}
          </select>
        </label>
        <label>
          <span class="label">Description</span>
          <textarea rows="3" bind:value={evDescription}></textarea>
        </label>
        <label>
          <span class="label">File</span>
          <input
            type="file"
            bind:this={evFileInput}
            onchange={onEvidenceFileChange}
            required
          />
          {#if evFile}
            <span class="faint hint">
              {evFile.name} · {fmtSize(evFile.size)}
            </span>
          {/if}
        </label>
        {#if evidenceUploadErr}
          <p class="form-err">{evidenceUploadErr}</p>
        {/if}
        <div class="form-actions">
          <button type="button" onclick={cancelEvidenceUpload} disabled={submittingEvidence}>
            Cancel
          </button>
          <button
            type="submit"
            class="primary"
            disabled={submittingEvidence || !evFile || !evTitle.trim()}
          >
            {submittingEvidence ? "Uploading…" : "Upload"}
          </button>
        </div>
      </form>
    {/if}

    {#if evidence.length === 0 && !showEvidenceUpload}
      <div class="card empty">
        <h3>No evidence yet</h3>
        <p class="muted">
          Evidence appears here automatically when you upload a data import or run
          a matcher. You can also upload screenshots, emails, and attestations
          directly.
        </p>
      </div>
    {:else if evidence.length > 0}
      <table>
        <thead>
          <tr>
            <th>Title</th>
            <th>Source</th>
            <th>File</th>
            <th>Test</th>
            <th>Size</th>
            <th>Obtained</th>
            <th class="actions-col"></th>
          </tr>
        </thead>
        <tbody>
          {#each evidence as ev (ev.id)}
            <tr>
              <td>
                <div>{ev.title}</div>
                {#if ev.description}
                  <div class="faint small">{ev.description}</div>
                {/if}
              </td>
              <td class="muted">{evidenceSourceLabel(ev.source)}</td>
              <td class="faint small">{ev.filename ?? "—"}</td>
              <td class="muted">{ev.test_code ?? "—"}</td>
              <td class="muted">{fmtSize(ev.plaintext_size)}</td>
              <td class="faint">{fmtDate(ev.obtained_at)}</td>
              <td class="actions-col">
                <button
                  type="button"
                  class="link"
                  onclick={() => downloadEvidence(ev)}
                  disabled={downloadingEvidenceId !== null}
                >
                  {downloadingEvidenceId === ev.id ? "Downloading…" : "Download"}
                </button>
              </td>
            </tr>
          {/each}
        </tbody>
      </table>
      {#if downloadErr}
        <p class="form-err">{downloadErr}</p>
      {/if}
    {/if}
  </section>
{/if}

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .back {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
    margin-bottom: var(--sp-3);
  }
  .back:hover { color: var(--text); }

  .block { margin-top: var(--sp-6); }
  .block h2 { margin: 0; }
  .block-head {
    display: flex;
    align-items: flex-start;
    justify-content: space-between;
    gap: var(--sp-4);
    margin-bottom: var(--sp-4);
  }
  .block-head p { margin-top: var(--sp-2); max-width: 62ch; }

  button {
    font: inherit;
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    border-radius: 2px;
  }
  button:hover:not(:disabled) { background: var(--accent-soft); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  button.primary {
    border-color: var(--accent);
    color: var(--accent);
  }

  .empty { max-width: 62ch; }
  .empty code {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--accent-soft);
    padding: 1px 6px;
    border-radius: 2px;
  }

  .form {
    max-width: 62ch;
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    margin-bottom: var(--sp-5);
  }
  .form h3 { margin: 0 0 var(--sp-2) 0; }
  .form label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .form input, .form select {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
  }
  .form input[type="file"] { padding: var(--sp-2); }
  .form textarea {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
    resize: vertical;
    min-height: 64px;
  }
  .form input:focus, .form select:focus, .form textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  .finding-form { max-width: none; margin: var(--sp-3) 0; }
  tr.editing td { background: var(--accent-soft); }
  .hint { font-size: 12px; }
  .form-actions {
    display: flex;
    gap: var(--sp-3);
    justify-content: flex-end;
    margin-top: var(--sp-2);
  }
  .form-err {
    color: #b04040;
    font-size: 13px;
    margin: 0;
  }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
  }
  th {
    text-align: left;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    font-size: 11px;
    font-weight: 500;
    color: var(--text-faint);
    border-bottom: 1px solid var(--border);
    padding: var(--sp-2) var(--sp-3);
  }
  td {
    padding: var(--sp-3);
    border-bottom: 1px solid var(--border);
    vertical-align: top;
  }
  tr:hover td { background: var(--accent-soft); }

  .actions-col {
    text-align: right;
    width: 140px;
  }
  .row-actions {
    display: flex;
    gap: var(--sp-3);
    justify-content: flex-end;
    align-items: center;
  }
  .link {
    background: transparent;
    border: 0;
    padding: 0;
    color: var(--accent);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }
  .link:hover:not(:disabled) { text-decoration: underline; }
  .link:disabled { opacity: 0.5; cursor: not-allowed; }

  .small { font-size: 11px; margin-top: 2px; }

  .pill {
    display: inline-block;
    font-size: 11px;
    padding: 2px 8px;
    border-radius: 10px;
    border: 1px solid var(--border);
    text-transform: uppercase;
    letter-spacing: 0.05em;
  }
  .pill-pass { color: #2d7a4a; border-color: #8fcfa2; }
  .pill-exception { color: #b07a2d; border-color: #e3c99b; }
  .pill-fail { color: #b04040; border-color: #e0a8a8; }
  .pill-sev-critical    { color: #8c1f1f; border-color: #d98888; }
  .pill-sev-high        { color: #b04040; border-color: #e0a8a8; }
  .pill-sev-medium      { color: #b07a2d; border-color: #e3c99b; }
  .pill-sev-low         { color: #2d7a4a; border-color: #8fcfa2; }
  .pill-sev-observation { color: var(--text-muted); border-color: var(--border); }

  code {
    font-family: var(--font-mono);
    font-size: 12px;
    background: var(--accent-soft);
    padding: 1px 6px;
    border-radius: 2px;
  }

  /* -------- Today / Overview block -------- */

  .overview { margin-top: var(--sp-4); }

  .overview-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(180px, 1fr));
    gap: var(--sp-4);
    margin-bottom: var(--sp-5);
  }
  .overview-card {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: var(--sp-4);
    background: var(--bg);
  }
  .overview-card .label {
    font-size: 11px;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .overview-card .stat {
    margin: var(--sp-2) 0 var(--sp-1) 0;
    font-family: var(--font-serif);
    font-size: 28px;
    font-weight: 400;
    line-height: 1;
    color: var(--text);
  }
  .overview-card .faint {
    font-size: 12px;
    margin: 0;
  }

  .overview-section { margin-top: var(--sp-5); }
  .overview-section h3 {
    margin: 0 0 var(--sp-3) 0;
    font-size: 14px;
    font-weight: 500;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-muted);
  }

  .risk-strip {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--sp-3);
  }
  .risk-card {
    border: 1px solid var(--border);
    border-left: 3px solid var(--text-faint);
    border-radius: 6px;
    padding: var(--sp-3) var(--sp-4);
    background: var(--bg);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .risk-card-head {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--sp-2);
  }
  .risk-card .label {
    font-size: 11px;
    color: var(--text-faint);
    text-transform: uppercase;
    letter-spacing: 0.06em;
  }
  .risk-card .risk-title {
    font-size: 13px;
    margin: 0;
    color: var(--text);
    line-height: 1.35;
  }
  .risk-card .coverage-state,
  .risk-card .risk-counts {
    font-size: 11px;
    margin: 0;
  }
  /* Coverage-state colour-codes the left rail. Low-saturation tones so the
     row reads as informational, not alarming. */
  .risk-card.cov-uncovered             { border-left-color: #b07a2d; }
  .risk-card.cov-untested              { border-left-color: var(--text-faint); }
  .risk-card.cov-tested_clean          { border-left-color: #2d7a4a; }
  .risk-card.cov-tested_with_exceptions{ border-left-color: #b04040; }

  /* Inherent-rating pills mirror the severity scale visually but use
     distinct class names so future restyling can fork them. */
  .pill-rating-high    { color: #b04040; border-color: #e0a8a8; }
  .pill-rating-medium  { color: #b07a2d; border-color: #e3c99b; }
  .pill-rating-low     { color: #2d7a4a; border-color: #8fcfa2; }

  .two-col {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: var(--sp-5);
    margin-top: var(--sp-5);
  }
  @media (max-width: 800px) {
    .two-col { grid-template-columns: 1fr; }
  }

  .attention-list,
  .activity-list {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .attention-item {
    display: flex;
    gap: var(--sp-3);
    align-items: flex-start;
    font-size: 13px;
    line-height: 1.45;
    padding: var(--sp-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .attention-item:last-child { border-bottom: 0; }
  .attention-item .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    margin-top: 6px;
    flex-shrink: 0;
    background: var(--text-faint);
  }
  .attention-item.priority-high .dot   { background: #b04040; }
  .attention-item.priority-medium .dot { background: #b07a2d; }
  .attention-item.priority-low .dot    { background: var(--text-faint); }
  .attention-label { flex: 1; }

  .activity-list li {
    display: flex;
    flex-direction: column;
    gap: 2px;
    font-size: 13px;
    line-height: 1.4;
    padding: var(--sp-2) 0;
    border-bottom: 1px solid var(--border);
  }
  .activity-list li:last-child { border-bottom: 0; }
  .activity-list .when { font-size: 11px; }
  .activity-list .what { font-size: 13px; }
  .activity-list .summary-line { font-size: 12px; }
</style>
