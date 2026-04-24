<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type EngagementSummary,
    type EvidenceSummary,
    type FindingSummary,
    type SeveritySummary,
    type TestResultSummary,
    type TestSummary,
  } from "../api/tauri";
  import FindingEditor from "../components/FindingEditor.svelte";
  import {
    currentEngagementId,
    currentRoute,
    currentTestId,
    openEngagement,
  } from "../stores/router";

  const MATCHER_ENABLED_CODES = new Set([
    "UAM-T-001",
    "UAM-T-003",
    "UAM-T-004",
    "CHG-T-001",
    "CHG-T-002",
    "BKP-T-001",
    "ITAC-T-001",
    "ITAC-T-002",
  ]);

  let loading = $state(true);
  let err = $state("");

  let engagement = $state<EngagementSummary | null>(null);
  let tests = $state<TestSummary[]>([]);
  let results = $state<TestResultSummary[]>([]);
  let findings = $state<FindingSummary[]>([]);
  let severities = $state<SeveritySummary[]>([]);
  let evidence = $state<EvidenceSummary[]>([]);

  let runningMatcher = $state(false);
  let runErr = $state("");

  let elevatingResultId = $state<string | null>(null);
  let elevateErr = $state("");

  let editingFindingId = $state<string | null>(null);

  let downloadingEvidenceId = $state<string | null>(null);
  let downloadErr = $state("");

  // Evidence upload.
  let showEvidenceUpload = $state(false);
  let submittingEvidence = $state(false);
  let evidenceUploadErr = $state("");
  let evTitle = $state("");
  let evDescription = $state("");
  let evObtainedFrom = $state("");
  let evFile = $state<File | null>(null);
  let evFileInput: HTMLInputElement | null = $state(null);

  const engagementIdValue = $derived($currentEngagementId);
  const testIdValue = $derived($currentTestId);

  const test = $derived(tests.find((t) => t.id === testIdValue) ?? null);

  const testResults = $derived(
    results
      .filter((r) => r.test_id === testIdValue)
      .sort((a, b) => b.performed_at - a.performed_at),
  );

  const testFindings = $derived(
    findings
      .filter((f) => f.test_id === testIdValue)
      .sort((a, b) => b.identified_at - a.identified_at),
  );

  const testEvidence = $derived(
    evidence
      .filter(
        (ev) =>
          ev.test_id === testIdValue ||
          (testIdValue != null && ev.linked_test_ids.includes(testIdValue)),
      )
      .sort((a, b) => b.obtained_at - a.obtained_at),
  );

  onMount(async () => {
    if (!engagementIdValue || !testIdValue) {
      currentRoute.set("engagements");
      return;
    }
    await load(engagementIdValue);
  });

  async function load(id: string) {
    loading = true;
    err = "";
    try {
      const [all, testList, resultList, findingList, severityList, evidenceList] =
        await Promise.all([
          api.listEngagements(),
          api.engagementListTests(id),
          api.engagementListTestResults(id),
          api.engagementListFindings(id),
          api.listFindingSeverities(),
          api.engagementListEvidence(id),
        ]);
      const match = all.find((e) => e.id === id);
      engagement = match ?? null;
      if (!match) {
        err = "Engagement not found.";
      }
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

  function backToEngagement() {
    if (engagement) {
      openEngagement(engagement.id);
    } else {
      currentTestId.set(null);
      currentRoute.set("engagements");
    }
  }

  async function runMatcher() {
    if (!engagement || !test || runningMatcher) return;
    runningMatcher = true;
    runErr = "";
    try {
      await api.engagementRunMatcher({
        test_id: test.id,
        overrides: null,
      });
      await load(engagement.id);
    } catch (e) {
      runErr = String(e);
    } finally {
      runningMatcher = false;
    }
  }

  async function elevate(result: TestResultSummary) {
    if (!engagement || elevatingResultId) return;
    elevatingResultId = result.id;
    elevateErr = "";
    try {
      await api.engagementElevateFinding({
        test_result_id: result.id,
        title: null,
        severity_id: null,
      });
      const [r, f] = await Promise.all([
        api.engagementListTestResults(engagement.id),
        api.engagementListFindings(engagement.id),
      ]);
      results = r;
      findings = f;
    } catch (e) {
      elevateErr = String(e);
    } finally {
      elevatingResultId = null;
    }
  }

  function onFindingSaved(updated: FindingSummary) {
    findings = findings.map((f) => (f.id === updated.id ? updated : f));
    editingFindingId = null;
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

  function openEvidenceUpload() {
    showEvidenceUpload = true;
    evidenceUploadErr = "";
    evTitle = "";
    evDescription = "";
    evObtainedFrom = "";
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
    if (!engagement || !test || !evFile) return;
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
        test_id: test.id,
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

  function parseSteps(steps: string | string[] | undefined): string[] {
    if (!steps) return [];
    if (Array.isArray(steps)) return steps;
    return [];
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

  function severityLabel(id: string | null): string {
    if (!id) return "—";
    const match = severities.find((s) => s.id === id);
    return match ? match.name : id;
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
</script>

<header>
  <button type="button" class="back" onclick={backToEngagement}>
    ← Back to engagement
  </button>
  {#if loading}
    <p class="faint">Loading…</p>
  {:else if err}
    <p class="faint">{err}</p>
  {:else if test && engagement}
    <span class="label">Working paper</span>
    <h1>{test.code} · {test.name}</h1>
    <p class="muted">
      {engagement.name} · {engagement.client_name}
    </p>
  {:else if !test}
    <p class="faint">Test not found in this engagement.</p>
  {/if}
</header>

<hr />

{#if !loading && !err && test && engagement}
  <div class="wp">
    <section class="block">
      <div class="block-head">
        <div>
          <span class="label">Control</span>
          <h2>{test.control_code} · {test.control_title}</h2>
        </div>
      </div>
      <div class="card prose">
        <p class="muted">
          This test is one procedure under control <code>{test.control_code}</code>.
          Control-level context, framework mappings, and sibling tests live on
          the engagement detail page.
        </p>
      </div>
    </section>

    <section class="block">
      <div class="block-head">
        <div>
          <span class="label">Test</span>
          <h2>Objective and procedure</h2>
        </div>
        {#if test.automation_tier === "rule_based" && MATCHER_ENABLED_CODES.has(test.code)}
          <button
            type="button"
            class="primary"
            onclick={runMatcher}
            disabled={runningMatcher}
          >
            {runningMatcher ? "Running…" : "Run matcher"}
          </button>
        {/if}
      </div>
      <div class="card prose">
        <div class="meta">
          <span><span class="label">Status</span> {test.status.replace(/_/g, " ")}</span>
          <span>
            <span class="label">Tier</span>
            {test.automation_tier.replace(/_/g, " ")}
          </span>
        </div>
        <p>{test.objective}</p>
      </div>
      {#if runErr}
        <p class="form-err">{runErr}</p>
      {/if}
    </section>

    <section class="block">
      <div class="block-head">
        <div>
          <span class="label">Results</span>
          <h2>Run history</h2>
          <p class="muted">
            Each run of the matcher (or each manual result you log) appears
            here newest first. Elevate an exception to a finding when
            management agrees there's an issue to remediate.
          </p>
        </div>
      </div>

      {#if testResults.length === 0}
        <div class="card empty">
          <h3>No results yet</h3>
          <p class="muted">
            Run the matcher or record a manual result to populate this section.
          </p>
        </div>
      {:else}
        <ol class="timeline">
          {#each testResults as r (r.id)}
            <li>
              <div class="tl-head">
                <span class="pill pill-{r.outcome}">{outcomeLabel(r.outcome)}</span>
                <span class="faint">{fmtDate(r.performed_at)}</span>
                <span class="faint">by {r.performed_by_name ?? "—"}</span>
              </div>
              {#if r.population_ref_label}
                <div class="faint small">Population: {r.population_ref_label}</div>
              {/if}
              {#if r.exception_summary}
                <p>{r.exception_summary}</p>
              {/if}
              <div class="tl-actions">
                {#if r.evidence_count > 0}
                  <span class="muted small">
                    {r.evidence_count} exception{r.evidence_count === 1 ? "" : "s"}
                  </span>
                {/if}
                {#if r.outcome === "exception" && !r.has_linked_finding}
                  <button
                    type="button"
                    class="link"
                    onclick={() => elevate(r)}
                    disabled={elevatingResultId !== null}
                  >
                    {elevatingResultId === r.id ? "Elevating…" : "Create finding"}
                  </button>
                {:else if r.has_linked_finding}
                  <span class="faint small">finding raised</span>
                {/if}
              </div>
            </li>
          {/each}
        </ol>
        {#if elevateErr}
          <p class="form-err">{elevateErr}</p>
        {/if}
      {/if}
    </section>

    <section class="block">
      <div class="block-head">
        <div>
          <span class="label">Findings</span>
          <h2>Draft findings for this test</h2>
          <p class="muted">
            Use CCCER to structure each finding — Condition, Criteria, Cause,
            Effect, Recommendation. The condition is facts; the recommendation
            addresses the cause, not just the visible instance.
          </p>
        </div>
      </div>

      {#if testFindings.length === 0}
        <div class="card empty">
          <h3>No findings yet</h3>
          <p class="muted">
            Findings appear here when you elevate an exception from a test
            result above.
          </p>
        </div>
      {:else}
        <div class="findings">
          {#each testFindings as f (f.id)}
            <article class="card finding">
              {#if editingFindingId === f.id}
                <FindingEditor
                  finding={f}
                  {severities}
                  onSaved={onFindingSaved}
                  onCancel={() => (editingFindingId = null)}
                />
              {:else}
                <header class="finding-head">
                  <div>
                    <code>{f.code}</code>
                    <strong>{f.title}</strong>
                  </div>
                  <div class="finding-meta">
                    <span class="pill pill-{f.severity_id ?? ''}">
                      {severityLabel(f.severity_id)}
                    </span>
                    <span class="faint small">
                      {f.status.replace(/_/g, " ")}
                    </span>
                  </div>
                </header>
                <dl class="cccer">
                  <div>
                    <dt>Condition</dt>
                    <dd>{f.condition_text ?? "—"}</dd>
                  </div>
                  <div>
                    <dt>Criteria</dt>
                    <dd class:missing={!f.criteria_text}>
                      {f.criteria_text ?? "Not yet recorded"}
                    </dd>
                  </div>
                  <div>
                    <dt>Cause</dt>
                    <dd class:missing={!f.cause_text}>
                      {f.cause_text ?? "Not yet recorded"}
                    </dd>
                  </div>
                  <div>
                    <dt>Effect</dt>
                    <dd class:missing={!f.effect_text}>
                      {f.effect_text ?? "Not yet recorded"}
                    </dd>
                  </div>
                  <div>
                    <dt>Recommendation</dt>
                    <dd>{f.recommendation_text ?? "—"}</dd>
                  </div>
                </dl>
                <footer class="finding-foot">
                  <span class="faint small">
                    Identified {fmtDate(f.identified_at)} by
                    {f.identified_by_name ?? "—"}
                  </span>
                  <button
                    type="button"
                    class="link"
                    onclick={() => (editingFindingId = f.id)}
                    disabled={editingFindingId !== null}
                  >
                    Edit
                  </button>
                </footer>
              {/if}
            </article>
          {/each}
        </div>
      {/if}
    </section>

    <section class="block">
      <div class="block-head">
        <div>
          <span class="label">Evidence</span>
          <h2>Supporting files for this test</h2>
          <p class="muted">
            Data imports, matcher reports, and auditor uploads linked to this
            test. Evidence is encrypted on disk and decrypted on download.
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
          <h3>Upload evidence for {test.code}</h3>
          <label>
            <span class="label">Title</span>
            <input type="text" bind:value={evTitle} required />
          </label>
          <label>
            <span class="label">Obtained from</span>
            <input
              type="text"
              bind:value={evObtainedFrom}
              placeholder="e.g. IT manager email"
            />
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
            <button
              type="button"
              onclick={cancelEvidenceUpload}
              disabled={submittingEvidence}
            >
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

      {#if testEvidence.length === 0 && !showEvidenceUpload}
        <div class="card empty">
          <h3>No evidence linked yet</h3>
          <p class="muted">
            When you upload a data import, run a matcher, or attach a file to
            this test, it appears here.
          </p>
        </div>
      {:else if testEvidence.length > 0}
        <table>
          <thead>
            <tr>
              <th>Title</th>
              <th>Source</th>
              <th>File</th>
              <th>Size</th>
              <th>Obtained</th>
              <th class="actions-col"></th>
            </tr>
          </thead>
          <tbody>
            {#each testEvidence as ev (ev.id)}
              <tr>
                <td>
                  <div>{ev.title}</div>
                  {#if ev.description}
                    <div class="faint small">{ev.description}</div>
                  {/if}
                </td>
                <td class="muted">{evidenceSourceLabel(ev.source)}</td>
                <td class="faint small">{ev.filename ?? "—"}</td>
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
  </div>
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

  .wp { display: flex; flex-direction: column; gap: var(--sp-6); }

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

  .card {
    border: 1px solid var(--border);
    border-radius: 8px;
    padding: var(--sp-4);
    background: var(--bg);
  }
  .prose p { margin: var(--sp-3) 0 0 0; max-width: 62ch; }
  .prose .meta {
    display: flex;
    gap: var(--sp-5);
    font-size: 12px;
  }
  .prose .meta .label { margin-right: var(--sp-2); }

  .empty { max-width: 62ch; }

  .timeline {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
  }
  .timeline li {
    border-left: 2px solid var(--border);
    padding: var(--sp-3) var(--sp-4);
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .timeline li:first-child { border-left-color: var(--accent); }
  .tl-head { display: flex; gap: var(--sp-3); align-items: center; flex-wrap: wrap; }
  .tl-actions {
    display: flex;
    gap: var(--sp-4);
    align-items: center;
    justify-content: space-between;
  }
  .timeline p { margin: 0; max-width: 62ch; }

  .findings { display: flex; flex-direction: column; gap: var(--sp-4); }
  .finding { display: flex; flex-direction: column; gap: var(--sp-3); }
  .finding-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--sp-4);
  }
  .finding-head strong { margin-left: var(--sp-3); }
  .finding-meta { display: flex; gap: var(--sp-3); align-items: center; }
  .finding-foot {
    display: flex;
    justify-content: space-between;
    align-items: center;
    border-top: 1px solid var(--border);
    padding-top: var(--sp-3);
    margin-top: var(--sp-2);
  }

  .cccer {
    display: flex;
    flex-direction: column;
    gap: var(--sp-3);
    margin: 0;
  }
  .cccer > div { display: grid; grid-template-columns: 140px 1fr; gap: var(--sp-3); }
  .cccer dt {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
    margin: 0;
    padding-top: 2px;
  }
  .cccer dd {
    margin: 0;
    font-size: 13px;
    max-width: 62ch;
  }
  .cccer dd.missing { color: var(--text-faint); font-style: italic; }

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
  .form input {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
  }
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
  .form input:focus,
  .form textarea:focus {
    outline: none;
    border-color: var(--accent);
  }
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

  .actions-col {
    text-align: right;
    width: 140px;
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

  .small { font-size: 11px; }
  .hint { font-size: 12px; }

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
