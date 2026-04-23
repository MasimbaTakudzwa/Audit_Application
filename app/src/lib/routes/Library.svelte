<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type LibraryControlDetail,
    type LibraryControlSummary,
    type LibraryRiskSummary,
    type LibraryVersion,
  } from "../api/tauri";

  type Tab = "controls" | "risks";
  type View = "list" | "detail";

  let lib = $state<LibraryVersion | null>(null);
  let controls = $state<LibraryControlSummary[]>([]);
  let risks = $state<LibraryRiskSummary[]>([]);
  let tab = $state<Tab>("controls");
  let view = $state<View>("list");
  let detail = $state<LibraryControlDetail | null>(null);
  let loadingDetail = $state(false);

  let filterFramework = $state<string>("");
  let filterSystemType = $state<string>("");
  let keyword = $state<string>("");

  let loading = $state(true);
  let err = $state<string>("");

  onMount(async () => {
    try {
      const [v, cs, rs] = await Promise.all([
        api.libraryVersion(),
        api.libraryListControls(),
        api.libraryListRisks(),
      ]);
      lib = v;
      controls = cs;
      risks = rs;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  const systemTypeOptions = $derived.by(() => {
    const set = new Set<string>();
    for (const c of controls) for (const s of c.applicable_system_types) set.add(s);
    for (const r of risks) for (const s of r.applicable_system_types) set.add(s);
    return Array.from(set).sort();
  });

  const filteredControls = $derived.by(() => {
    const k = keyword.trim().toLowerCase();
    return controls.filter((c) => {
      if (filterFramework && !c.frameworks.includes(filterFramework)) return false;
      if (filterSystemType && !c.applicable_system_types.includes(filterSystemType))
        return false;
      if (k && !c.code.toLowerCase().includes(k) && !c.title.toLowerCase().includes(k))
        return false;
      return true;
    });
  });

  const filteredRisks = $derived.by(() => {
    const k = keyword.trim().toLowerCase();
    return risks.filter((r) => {
      if (filterSystemType && !r.applicable_system_types.includes(filterSystemType))
        return false;
      if (k && !r.code.toLowerCase().includes(k) && !r.title.toLowerCase().includes(k))
        return false;
      return true;
    });
  });

  async function openControl(id: string) {
    loadingDetail = true;
    err = "";
    try {
      detail = await api.libraryGetControl(id);
      view = "detail";
    } catch (e) {
      err = String(e);
    } finally {
      loadingDetail = false;
    }
  }

  function backToList() {
    view = "list";
    detail = null;
  }

  function prettySystemType(code: string): string {
    if (code === "generic-erp") return "Generic ERP";
    if (code === "core-banking") return "Core banking";
    return code;
  }
</script>

<header>
  <span class="label">Risk and control methodology</span>
  <h1>Library</h1>
  <p class="muted">
    Industry-baseline risks, controls, and test procedures. Firm-level overrides layer on top and
    survive library updates.
  </p>
</header>

<hr />

{#if loading}
  <p class="faint">Loading…</p>
{:else if err}
  <p class="form-err">{err}</p>
{:else if view === "detail" && detail}
  <div class="actions">
    <button type="button" onclick={backToList}>Back to library</button>
  </div>

  <article class="detail">
    <div class="detail-head">
      <span class="code">{detail.code}</span>
      <h2>{detail.title}</h2>
      <p class="pill-row">
        <span class="pill">{detail.control_type}</span>
        {#if detail.frequency}<span class="pill">{detail.frequency}</span>{/if}
        {#each detail.applicable_system_types as s}
          <span class="pill subtle">{prettySystemType(s)}</span>
        {/each}
      </p>
    </div>

    <section class="detail-section">
      <h3 class="section-label">Objective</h3>
      <p>{detail.objective}</p>
    </section>

    <section class="detail-section">
      <h3 class="section-label">Description</h3>
      <p>{detail.description}</p>
    </section>

    {#if detail.framework_mappings.length > 0}
      <section class="detail-section">
        <h3 class="section-label">Framework mappings</h3>
        <ul class="mappings">
          {#each detail.framework_mappings as m}
            <li><span class="fw">{m.framework}</span> <span class="ref">{m.reference}</span></li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if detail.related_risks.length > 0}
      <section class="detail-section">
        <h3 class="section-label">Related risks</h3>
        <ul class="risks">
          {#each detail.related_risks as r}
            <li>
              <span class="code">{r.code}</span>
              <span>{r.title}</span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if detail.test_procedures.length > 0}
      <section class="detail-section">
        <h3 class="section-label">Test procedures</h3>
        {#each detail.test_procedures as tp}
          <article class="tp">
            <div class="tp-head">
              <span class="code">{tp.code}</span>
              <h4>{tp.name}</h4>
            </div>
            <p class="muted">{tp.objective}</p>
            {#if tp.steps.length > 0}
              <ol class="steps">
                {#each tp.steps as step}<li>{step}</li>{/each}
              </ol>
            {/if}
            {#if tp.evidence_checklist.length > 0}
              <div class="checklist">
                <span class="section-label">Expected evidence</span>
                <ul>
                  {#each tp.evidence_checklist as item}<li>{item}</li>{/each}
                </ul>
              </div>
            {/if}
            <p class="tp-meta faint">
              Sampling: {tp.sampling_default} · Automation: {tp.automation_hint}
            </p>
          </article>
        {/each}
      </section>
    {/if}

    <p class="faint version-note">Library version {detail.library_version}</p>
  </article>
{:else}
  <section class="overview">
    {#if lib && lib.version}
      <p class="version-line">
        <span class="label">Current version</span>
        <span class="version">{lib.version}</span>
        <span class="frameworks">
          {#each lib.frameworks as f, i}
            <span>{f}</span>{#if i < lib.frameworks.length - 1}<span class="sep"> · </span>{/if}
          {/each}
        </span>
      </p>
    {:else}
      <p class="faint">No library installed.</p>
    {/if}
  </section>

  <nav class="tabs">
    <button
      type="button"
      class="tab"
      class:active={tab === "controls"}
      onclick={() => (tab = "controls")}
    >
      Controls <span class="count">{controls.length}</span>
    </button>
    <button
      type="button"
      class="tab"
      class:active={tab === "risks"}
      onclick={() => (tab = "risks")}
    >
      Risks <span class="count">{risks.length}</span>
    </button>
  </nav>

  <div class="filters">
    <label>
      <span class="label">Framework</span>
      <select bind:value={filterFramework} disabled={tab === "risks"}>
        <option value="">Any</option>
        {#if lib}
          {#each lib.frameworks as f}<option value={f}>{f}</option>{/each}
        {/if}
      </select>
    </label>

    <label>
      <span class="label">System type</span>
      <select bind:value={filterSystemType}>
        <option value="">Any</option>
        {#each systemTypeOptions as s}
          <option value={s}>{prettySystemType(s)}</option>
        {/each}
      </select>
    </label>

    <label class="keyword">
      <span class="label">Search</span>
      <input type="text" bind:value={keyword} placeholder="code or title" autocomplete="off" />
    </label>
  </div>

  {#if tab === "controls"}
    {#if filteredControls.length === 0}
      <p class="faint empty">No controls match the current filters.</p>
    {:else}
      <table>
        <thead>
          <tr>
            <th>Code</th>
            <th>Title</th>
            <th>Type</th>
            <th>Frequency</th>
            <th>Frameworks</th>
            <th>Tests</th>
          </tr>
        </thead>
        <tbody>
          {#each filteredControls as c (c.id)}
            <tr onclick={() => openControl(c.id)} class:disabled={loadingDetail}>
              <td class="code">{c.code}</td>
              <td>{c.title}</td>
              <td class="muted">{c.control_type}</td>
              <td class="muted">{c.frequency ?? "—"}</td>
              <td class="muted">{c.frameworks.join(" · ")}</td>
              <td class="faint">{c.test_procedure_count}</td>
            </tr>
          {/each}
        </tbody>
      </table>
    {/if}
  {:else if filteredRisks.length === 0}
    <p class="faint empty">No risks match the current filters.</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Code</th>
          <th>Title</th>
          <th>Inherent rating</th>
          <th>System types</th>
        </tr>
      </thead>
      <tbody>
        {#each filteredRisks as r (r.id)}
          <tr class="readonly">
            <td class="code">{r.code}</td>
            <td>{r.title}</td>
            <td class="muted">{r.default_inherent_rating ?? "—"}</td>
            <td class="muted">
              {r.applicable_system_types.map(prettySystemType).join(" · ")}
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
{/if}

<style>
  header {
    margin-bottom: var(--sp-5);
  }
  header h1 {
    margin-top: var(--sp-2);
  }
  header p {
    margin-top: var(--sp-3);
    max-width: 62ch;
  }

  .overview {
    margin-bottom: var(--sp-5);
  }
  .version-line {
    display: flex;
    align-items: baseline;
    gap: var(--sp-3);
    flex-wrap: wrap;
  }
  .version {
    font-family: var(--font-serif);
    font-size: 20px;
  }
  .frameworks {
    color: var(--text-muted);
    font-size: 13px;
  }
  .sep {
    color: var(--text-faint);
  }

  .tabs {
    display: flex;
    gap: var(--sp-4);
    border-bottom: 1px solid var(--border);
    margin-bottom: var(--sp-4);
  }
  .tab {
    font: inherit;
    background: none;
    border: none;
    padding: var(--sp-2) 0;
    margin-bottom: -1px;
    border-bottom: 1px solid transparent;
    color: var(--text-muted);
    cursor: pointer;
  }
  .tab:hover {
    color: var(--text);
  }
  .tab.active {
    color: var(--text);
    border-bottom-color: var(--accent);
  }
  .count {
    color: var(--text-faint);
    font-size: 12px;
    margin-left: var(--sp-2);
  }

  .filters {
    display: flex;
    gap: var(--sp-4);
    flex-wrap: wrap;
    margin-bottom: var(--sp-4);
  }
  .filters label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .filters .keyword {
    flex: 1;
    min-width: 220px;
  }
  .filters select,
  .filters input {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
  }
  .filters select:focus,
  .filters input:focus {
    outline: none;
    border-color: var(--accent);
  }

  .empty {
    margin-top: var(--sp-4);
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
  tbody tr {
    cursor: pointer;
  }
  tbody tr.readonly {
    cursor: default;
  }
  tbody tr:hover:not(.readonly) td {
    background: var(--accent-soft);
  }
  tr.disabled {
    opacity: 0.5;
    pointer-events: none;
  }
  .code {
    font-family: var(--font-mono);
    font-size: 12px;
    color: var(--text-muted);
    white-space: nowrap;
  }

  .actions {
    margin-bottom: var(--sp-4);
  }
  button {
    font: inherit;
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    border-radius: 2px;
  }
  button:hover:not(:disabled) {
    background: var(--accent-soft);
  }

  .form-err {
    color: #b04040;
    font-size: 13px;
  }

  .detail {
    max-width: 72ch;
  }
  .detail-head {
    margin-bottom: var(--sp-5);
  }
  .detail-head h2 {
    font-family: var(--font-serif);
    margin: var(--sp-2) 0 0 0;
  }
  .detail-section {
    margin-bottom: var(--sp-5);
  }
  .section-label {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.06em;
    color: var(--text-faint);
    font-weight: 500;
    margin: 0 0 var(--sp-2) 0;
  }
  .detail-section p {
    margin: 0;
    line-height: 1.55;
  }

  .pill-row {
    display: flex;
    gap: var(--sp-2);
    flex-wrap: wrap;
    margin-top: var(--sp-3);
  }
  .pill {
    display: inline-block;
    padding: 2px 8px;
    border: 1px solid var(--border);
    border-radius: 10px;
    font-size: 11px;
    color: var(--text-muted);
  }
  .pill.subtle {
    background: var(--accent-soft);
    border-color: transparent;
  }

  .mappings,
  .risks {
    list-style: none;
    padding: 0;
    margin: 0;
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .mappings li {
    display: flex;
    gap: var(--sp-3);
    align-items: baseline;
  }
  .fw {
    color: var(--text-muted);
    min-width: 10ch;
  }
  .ref {
    font-family: var(--font-mono);
    font-size: 12px;
  }
  .risks li {
    display: flex;
    gap: var(--sp-3);
    align-items: baseline;
  }

  .tp {
    border-top: 1px solid var(--border);
    padding-top: var(--sp-4);
    margin-top: var(--sp-4);
  }
  .tp-head {
    display: flex;
    gap: var(--sp-3);
    align-items: baseline;
  }
  .tp-head h4 {
    margin: 0;
    font-weight: 500;
  }
  .steps {
    margin: var(--sp-3) 0 0 var(--sp-5);
    padding: 0;
  }
  .steps li {
    margin-bottom: var(--sp-2);
    line-height: 1.55;
  }
  .checklist {
    margin-top: var(--sp-4);
  }
  .checklist ul {
    margin: var(--sp-2) 0 0 var(--sp-5);
    padding: 0;
  }
  .checklist li {
    margin-bottom: var(--sp-1);
  }
  .tp-meta {
    margin-top: var(--sp-3);
    font-size: 12px;
  }
  .version-note {
    margin-top: var(--sp-6);
    font-size: 12px;
  }
</style>
