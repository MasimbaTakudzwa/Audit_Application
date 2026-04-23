<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type DataImportSummary,
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
    | "payroll"
    | "badge_log"
    | "other";

  const PURPOSE_OPTIONS: { id: PurposeTag; label: string; hint: string }[] = [
    { id: "ad_export",   label: "AD export",       hint: "Active Directory user dump (enabled + disabled)" },
    { id: "entra_export",label: "Entra export",    hint: "Azure AD / Entra user dump" },
    { id: "hr_active",   label: "HR active list",  hint: "Employees currently on payroll" },
    { id: "hr_leavers",  label: "HR leavers list", hint: "Terminated or departed employees" },
    { id: "payroll",     label: "Payroll run",     hint: "Payroll register for the period" },
    { id: "badge_log",   label: "Badge / access log", hint: "Physical access or entry log" },
    { id: "other",       label: "Other",           hint: "Any other evidence you want to retain" },
  ];

  // Test codes for which the backend has a dispatchable matcher today. Keep
  // this list in sync with `AccessReviewRule::for_test_code` in Rust — any
  // code in here must also be dispatchable there, or the button will error
  // when pressed.
  const MATCHER_ENABLED_CODES = new Set(["UAM-T-001", "UAM-T-003"]);

  let loading = $state(true);
  let err = $state("");

  let engagement = $state<EngagementSummary | null>(null);
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
      const [all, dataImports, testList, resultList, findingList, severityList, evidenceList] =
        await Promise.all([
          api.listEngagements(),
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
    } catch (e) {
      addControlErr = String(e);
    } finally {
      addingControl = false;
    }
  }

  function onFindingSaved(updated: FindingSummary) {
    findings = findings.map((f) => (f.id === updated.id ? updated : f));
    editingFindingId = null;
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
      await api.engagementRunAccessReview({
        test_id: test.id,
        ad_import_id: null,
        leavers_import_id: null,
      });
      const [refreshedTests, refreshedResults, refreshedEvidence] = await Promise.all([
        api.engagementListTests(engagement.id),
        api.engagementListTestResults(engagement.id),
        api.engagementListEvidence(engagement.id),
      ]);
      tests = refreshedTests;
      results = refreshedResults;
      evidence = refreshedEvidence;
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
</style>
