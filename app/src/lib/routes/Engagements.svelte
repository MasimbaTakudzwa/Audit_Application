<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type ClientSummary,
    type EngagementSummary,
  } from "../api/tauri";
  import { openEngagement } from "../stores/router";

  let engagements = $state<EngagementSummary[]>([]);
  let clients = $state<ClientSummary[]>([]);
  let loading = $state(true);
  let err = $state<string>("");

  let showForm = $state(false);
  let submitting = $state(false);
  let formErr = $state<string>("");
  let clientId = $state("");
  let name = $state("");
  let fiscalYearLabel = $state("");
  let periodStart = $state("");
  let periodEnd = $state("");

  onMount(async () => {
    try {
      const [es, cs] = await Promise.all([
        api.listEngagements(),
        api.listClients(),
      ]);
      engagements = es;
      clients = cs;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  function openForm() {
    showForm = true;
    formErr = "";
    clientId = clients.length > 0 ? clients[0].id : "";
    name = "";
    fiscalYearLabel = "";
    periodStart = "";
    periodEnd = "";
  }

  function cancelForm() {
    showForm = false;
    formErr = "";
  }

  async function submit(event: Event) {
    event.preventDefault();
    formErr = "";
    submitting = true;
    try {
      const created = await api.createEngagement({
        client_id: clientId,
        name,
        fiscal_year_label: fiscalYearLabel || null,
        period_start: periodStart || null,
        period_end: periodEnd || null,
      });
      engagements = [created, ...engagements];
      showForm = false;
    } catch (e) {
      formErr = String(e);
    } finally {
      submitting = false;
    }
  }

  function fmt(ts: number) {
    return new Date(ts * 1000).toLocaleDateString("en-GB", {
      day: "numeric",
      month: "short",
      year: "numeric",
    });
  }
</script>

<header>
  <span class="label">Active and archived audits</span>
  <h1>Engagements</h1>
  <p class="muted">
    Each engagement is a self-contained snapshot. The library version is frozen at creation time so
    methodology stays stable across library updates.
  </p>
  {#if !loading && !err && !showForm}
    <div class="actions">
      <button
        type="button"
        class="primary"
        onclick={openForm}
        disabled={clients.length === 0}
      >
        New engagement
      </button>
      {#if clients.length === 0}
        <span class="faint hint">Create a client first to scope an engagement.</span>
      {/if}
    </div>
  {/if}
</header>

<hr />

{#if loading}
  <p class="faint">Loading…</p>
{:else if err}
  <p class="faint">{err}</p>
{:else}
  {#if showForm}
    <form class="card form" onsubmit={submit}>
      <h3>New engagement</h3>

      <label>
        <span class="label">Client</span>
        <select bind:value={clientId} required>
          {#each clients as c (c.id)}
            <option value={c.id}>{c.name}</option>
          {/each}
        </select>
      </label>

      <label>
        <span class="label">Engagement name</span>
        <input
          type="text"
          bind:value={name}
          required
          maxlength="200"
          placeholder="e.g. FY2026 IT general controls audit"
          autocomplete="off"
        />
      </label>

      <div class="row">
        <label>
          <span class="label">Fiscal year</span>
          <input
            type="text"
            bind:value={fiscalYearLabel}
            maxlength="40"
            placeholder="FY2026"
          />
        </label>

        <label>
          <span class="label">Period start</span>
          <input type="date" bind:value={periodStart} />
        </label>

        <label>
          <span class="label">Period end</span>
          <input type="date" bind:value={periodEnd} />
        </label>
      </div>

      <p class="faint hint">
        Fiscal year + start + end must be provided together, or all left blank.
      </p>

      {#if formErr}
        <p class="form-err">{formErr}</p>
      {/if}

      <div class="form-actions">
        <button type="button" onclick={cancelForm} disabled={submitting}>Cancel</button>
        <button type="submit" class="primary" disabled={submitting}>
          {submitting ? "Creating…" : "Create engagement"}
        </button>
      </div>
    </form>
  {/if}

  {#if engagements.length === 0 && !showForm}
    <div class="card empty">
      <h3>No engagements yet</h3>
      <p class="muted">
        An engagement is a self-contained snapshot of a year's work: library version, per-engagement
        encryption key, team, and scope all frozen at creation.
      </p>
    </div>
  {:else if engagements.length > 0}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Client</th>
          <th>Period</th>
          <th>Status</th>
          <th>Created</th>
          <th class="actions-col"></th>
        </tr>
      </thead>
      <tbody>
        {#each engagements as e (e.id)}
          <tr>
            <td>{e.name}</td>
            <td class="muted">{e.client_name}</td>
            <td class="muted">{e.fiscal_year ?? "—"}</td>
            <td class="accent">{e.status}</td>
            <td class="faint">{fmt(e.created_at)}</td>
            <td class="actions-col">
              <button type="button" class="link" onclick={() => openEngagement(e.id)}>
                Open →
              </button>
            </td>
          </tr>
        {/each}
      </tbody>
    </table>
  {/if}
{/if}

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .actions {
    margin-top: var(--sp-4);
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .hint { font-size: 12px; }

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
  .form .row {
    display: grid;
    grid-template-columns: 1fr 1fr 1fr;
    gap: var(--sp-3);
  }
  .form input, .form select {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
  }
  .form input:focus, .form select:focus {
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
  }
  tr:hover td { background: var(--accent-soft); }

  .actions-col {
    text-align: right;
    width: 96px;
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
  .link:hover { text-decoration: underline; }
</style>
