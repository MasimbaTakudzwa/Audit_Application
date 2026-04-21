<script lang="ts">
  import type { Snippet } from "svelte";
  import { currentRoute, type RouteId } from "../stores/router";
  import { theme, toggleTheme } from "../stores/theme";

  type Props = { children: Snippet };
  let { children }: Props = $props();

  const nav: { id: RouteId; label: string }[] = [
    { id: "dashboard",   label: "Dashboard" },
    { id: "clients",     label: "Clients" },
    { id: "engagements", label: "Engagements" },
    { id: "library",     label: "Library" },
    { id: "settings",    label: "Settings" },
  ];
</script>

<div class="shell">
  <aside class="sidebar">
    <div class="brand">
      <span class="brand-name">Audit</span>
      <span class="brand-sub">workspace</span>
    </div>

    <nav aria-label="Primary">
      {#each nav as item (item.id)}
        <button
          class="nav-item"
          class:active={$currentRoute === item.id}
          onclick={() => currentRoute.set(item.id)}
          type="button"
        >
          {item.label}
        </button>
      {/each}
    </nav>

    <div class="footer">
      <button class="theme-toggle" onclick={toggleTheme} type="button">
        {$theme === "dark" ? "Light" : "Dark"} mode
      </button>
      <span class="version faint">v0.1.0</span>
    </div>
  </aside>

  <main class="main">
    {@render children()}
  </main>
</div>

<style>
  .shell {
    display: grid;
    grid-template-columns: 232px 1fr;
    height: 100%;
    background: var(--bg);
  }

  .sidebar {
    background: var(--surface);
    border-right: 1px solid var(--border);
    padding: var(--sp-7) var(--sp-4) var(--sp-5);
    display: flex;
    flex-direction: column;
    gap: var(--sp-6);
  }

  .brand {
    display: flex;
    flex-direction: column;
    padding-left: var(--sp-2);
  }
  .brand-name {
    font-family: var(--font-serif);
    font-size: 22px;
    line-height: 1;
    font-weight: 500;
  }
  .brand-sub {
    font-size: 11px;
    text-transform: uppercase;
    letter-spacing: 0.1em;
    color: var(--text-faint);
    margin-top: 3px;
  }

  nav {
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .nav-item {
    text-align: left;
    background: transparent;
    border: 0;
    padding: var(--sp-2) var(--sp-3);
    color: var(--text-muted);
    border-radius: var(--radius-sm);
    cursor: pointer;
    transition: background-color 120ms var(--ease), color 120ms var(--ease);
  }
  .nav-item:hover {
    background: var(--accent-soft);
    color: var(--text);
  }
  .nav-item.active {
    color: var(--text);
    background: var(--accent-soft);
  }

  .footer {
    margin-top: auto;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .theme-toggle {
    background: transparent;
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-3);
    color: var(--text-muted);
    cursor: pointer;
    transition: border-color 120ms var(--ease), color 120ms var(--ease);
  }
  .theme-toggle:hover {
    border-color: var(--border-strong);
    color: var(--text);
  }
  .version {
    font-family: var(--font-mono);
    font-size: 11px;
  }

  .main {
    padding: var(--sp-7) var(--sp-8) var(--sp-8);
    overflow-y: auto;
  }
</style>
