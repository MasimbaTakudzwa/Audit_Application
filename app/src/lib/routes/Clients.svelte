<script lang="ts">
  import { onMount } from "svelte";
  import { api, type ClientSummary } from "../api/tauri";

  let clients = $state<ClientSummary[]>([]);
  let loading = $state(true);
  let err = $state<string>("");

  onMount(async () => {
    try {
      clients = await api.listClients();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });
</script>

<header>
  <span class="label">Organisations being audited</span>
  <h1>Clients</h1>
  <p class="muted">Distinct from client-portal users. Each client has one or more engagements across years.</p>
</header>

<hr />

{#if loading}
  <p class="faint">Loading…</p>
{:else if err}
  <p class="faint">{err}</p>
{:else if clients.length === 0}
  <div class="card empty">
    <h3>No clients yet</h3>
    <p class="muted">
      Create your first client to start scoping an engagement. Industry selection pre-populates
      common system types for faster scoping.
    </p>
    <p class="faint">Create-client form lands with the auth flow.</p>
  </div>
{:else}
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

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .empty { max-width: 62ch; }

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
