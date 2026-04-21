<script lang="ts">
  import { onMount } from "svelte";
  import { api, type EngagementSummary } from "../api/tauri";

  let engagements = $state<EngagementSummary[]>([]);
  let loading = $state(true);
  let err = $state<string>("");

  onMount(async () => {
    try {
      engagements = await api.listEngagements();
    } catch (e) {
      err = String(e);
    } finally {
      loading = false;
    }
  });

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
  <p class="muted">Each engagement is a self-contained snapshot. New engagements clone methodology from the prior year via the <code>derived_from</code> chain.</p>
</header>

<hr />

{#if loading}
  <p class="faint">Loading…</p>
{:else if err}
  <p class="faint">{err}</p>
{:else if engagements.length === 0}
  <div class="card empty">
    <h3>No engagements yet</h3>
    <p class="muted">
      An engagement pulls in the client's prior methodology, scoped systems, and team assignments.
      The first one creates library-originated records; subsequent ones derive from the previous.
    </p>
    <p class="faint">Engagement creation flow is the next vertical slice.</p>
  </div>
{:else}
  <table>
    <thead>
      <tr>
        <th>Name</th>
        <th>Client</th>
        <th>Period</th>
        <th>Status</th>
        <th>Created</th>
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
        </tr>
      {/each}
    </tbody>
  </table>
{/if}

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }
  header code { background: var(--surface-sunken); padding: 1px 6px; border-radius: 3px; }

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
