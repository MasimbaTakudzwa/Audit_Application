<script lang="ts">
  import { onMount } from "svelte";
  import { api, type HealthStatus } from "../api/tauri";
  import { authView } from "../stores/auth";

  let status = $state<HealthStatus | null>(null);
  let err = $state<string>("");

  onMount(async () => {
    try {
      status = await api.ping();
    } catch (e) {
      err = String(e);
    }
  });

  let session = $derived(
    $authView.state === "signed_in" ? $authView.session : null,
  );
</script>

<header>
  <span class="label">Overview</span>
  <h1>Dashboard</h1>
  <p class="muted">Engagements in flight, open findings, evidence queue, review backlog.</p>
</header>

<hr />

<section class="status-grid">
  <div class="card">
    <span class="label">Backend</span>
    <p class="stat">
      {#if err}
        <span class="dot err"></span>Offline
      {:else if status}
        <span class="dot ok"></span>{status.app} <span class="faint">v{status.version}</span>
      {:else}
        <span class="dot"></span>Connecting
      {/if}
    </p>
    {#if err}<p class="faint error-msg">{err}</p>{/if}
  </div>

  <div class="card">
    <span class="label">Session</span>
    <p class="stat">
      {#if session}
        {session.display_name}
      {:else}
        <span class="faint">Not signed in</span>
      {/if}
    </p>
    {#if session}
      <p class="faint">{session.email}</p>
    {:else}
      <p class="faint">Sign in to continue.</p>
    {/if}
  </div>

  <div class="card">
    <span class="label">Database</span>
    <p class="stat">Initialised</p>
    <p class="faint">SQLite, seven migrations applied at launch.</p>
  </div>
</section>

<section class="coming-next">
  <h3>Coming next</h3>
  <ul>
    <li>Create your first firm, user, and sample client</li>
    <li>Open an engagement and scope its systems</li>
    <li>Run the user access review vertical slice end-to-end</li>
  </ul>
</section>

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .status-grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
    gap: var(--sp-4);
    margin-bottom: var(--sp-7);
  }
  .stat {
    font-family: var(--font-serif);
    font-size: 20px;
    margin: var(--sp-2) 0;
    display: flex;
    align-items: center;
    gap: var(--sp-3);
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--text-faint);
    display: inline-block;
  }
  .dot.ok  { background: var(--ok); }
  .dot.err { background: var(--err); }
  .error-msg { font-family: var(--font-mono); font-size: 12px; margin-top: var(--sp-2); }

  .coming-next {
    max-width: 62ch;
  }
  .coming-next ul {
    margin-top: var(--sp-3);
    padding-left: var(--sp-5);
    color: var(--text-muted);
  }
  .coming-next li + li {
    margin-top: var(--sp-2);
  }
</style>
