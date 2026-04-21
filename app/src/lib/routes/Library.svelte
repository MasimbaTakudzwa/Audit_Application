<script lang="ts">
  import { onMount } from "svelte";
  import { api, type LibraryVersion } from "../api/tauri";

  let lib = $state<LibraryVersion | null>(null);
  let err = $state<string>("");

  onMount(async () => {
    try {
      lib = await api.libraryVersion();
    } catch (e) {
      err = String(e);
    }
  });
</script>

<header>
  <span class="label">Risk and control methodology</span>
  <h1>Library</h1>
  <p class="muted">Industry-baseline risks, controls, and test procedures. Firm-level overrides layer on top and survive library updates.</p>
</header>

<hr />

<section class="card overview">
  <span class="label">Current version</span>
  {#if lib}
    <p class="version-line">
      <span class="version">{lib.version}</span>
      <span class="frameworks">
        {#each lib.frameworks as f, i}
          <span>{f}</span>{#if i < lib.frameworks.length - 1} <span class="sep">·</span> {/if}
        {/each}
      </span>
    </p>
  {:else if err}
    <p class="faint">{err}</p>
  {:else}
    <p class="faint">Loading…</p>
  {/if}
</section>

<section class="placeholder">
  <h3>Library browser</h3>
  <p class="muted">
    Browse risks, controls, and test procedures by framework, system type, or keyword. Firm overrides
    are highlighted inline. Not implemented in scaffold.
  </p>
</section>

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .overview { max-width: 62ch; margin-bottom: var(--sp-6); }
  .version-line {
    display: flex;
    align-items: baseline;
    gap: var(--sp-4);
    margin-top: var(--sp-2);
    flex-wrap: wrap;
  }
  .version {
    font-family: var(--font-serif);
    font-size: 22px;
  }
  .frameworks {
    color: var(--text-muted);
    font-size: 13px;
  }
  .sep { color: var(--text-faint); }

  .placeholder { max-width: 62ch; }
  .placeholder h3 { margin-bottom: var(--sp-3); }
</style>
