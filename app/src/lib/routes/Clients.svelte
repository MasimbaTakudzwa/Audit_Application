<script lang="ts">
  import { onMount } from "svelte";
  import {
    api,
    type ClientSummary,
    type IndustrySummary,
  } from "../api/tauri";

  let clients = $state<ClientSummary[]>([]);
  let industries = $state<IndustrySummary[]>([]);
  let loading = $state(true);
  let err = $state<string>("");

  let showForm = $state(false);
  let submitting = $state(false);
  let formErr = $state<string>("");
  let name = $state("");
  let country = $state("Zimbabwe");
  let industryId = $state("");

  onMount(async () => {
    try {
      const [cs, is] = await Promise.all([
        api.listClients(),
        api.listIndustries(),
      ]);
      clients = cs;
      industries = is;
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

  function openForm() {
    showForm = true;
    formErr = "";
    name = "";
    country = "Zimbabwe";
    industryId = "";
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
      const created = await api.createClient({
        name,
        country,
        industry_id: industryId === "" ? null : industryId,
      });
      clients = [...clients, created].sort((a, b) =>
        a.name.localeCompare(b.name),
      );
      showForm = false;
    } catch (e) {
      formErr = String(e);
    } finally {
      submitting = false;
    }
  }
</script>

<header>
  <span class="label">Organisations being audited</span>
  <h1>Clients</h1>
  <p class="muted">
    Distinct from client-portal users. Each client has one or more engagements across years.
  </p>
  {#if !loading && !err && !showForm}
    <div class="actions">
      <button type="button" class="primary" onclick={openForm}>New client</button>
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
      <h3>New client</h3>

      <label>
        <span class="label">Name</span>
        <input
          type="text"
          bind:value={name}
          required
          maxlength="200"
          placeholder="e.g. Zimbabwe National Water Authority"
          autocomplete="off"
        />
      </label>

      <label>
        <span class="label">Country</span>
        <input type="text" bind:value={country} required maxlength="80" />
      </label>

      <label>
        <span class="label">Industry</span>
        <select bind:value={industryId}>
          <option value="">— not set —</option>
          {#each industries as i (i.id)}
            <option value={i.id}>{i.name}</option>
          {/each}
        </select>
      </label>

      {#if formErr}
        <p class="form-err">{formErr}</p>
      {/if}

      <div class="form-actions">
        <button type="button" onclick={cancelForm} disabled={submitting}>Cancel</button>
        <button type="submit" class="primary" disabled={submitting}>
          {submitting ? "Creating…" : "Create client"}
        </button>
      </div>
    </form>
  {/if}

  {#if clients.length === 0 && !showForm}
    <div class="card empty">
      <h3>No clients yet</h3>
      <p class="muted">
        Create your first client to start scoping an engagement. Industry selection pre-populates
        common system types for faster scoping.
      </p>
    </div>
  {:else if clients.length > 0}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Industry</th>
          <th>Country</th>
          <th>Status</th>
        </tr>
      </thead>
      <tbody>
        {#each clients as c (c.id)}
          <tr>
            <td>{c.name}</td>
            <td class="muted">{c.industry ?? "—"}</td>
            <td class="muted">{c.country}</td>
            <td class="faint">{c.status}</td>
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

  .actions { margin-top: var(--sp-4); }

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
    max-width: 52ch;
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
</style>
