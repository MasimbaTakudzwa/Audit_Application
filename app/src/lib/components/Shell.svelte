<script lang="ts">
  import type { Snippet } from "svelte";
  import { currentRoute, type RouteId } from "../stores/router";
  import { theme, toggleTheme } from "../stores/theme";
  import { authView, logout } from "../stores/auth";

  type Props = { children: Snippet };
  let { children }: Props = $props();

  const nav: { id: RouteId; label: string }[] = [
    { id: "dashboard",   label: "Dashboard" },
    { id: "clients",     label: "Clients" },
    { id: "engagements", label: "Engagements" },
    { id: "library",     label: "Library" },
    { id: "settings",    label: "Settings" },
  ];

  let signingOut = $state(false);
  async function handleLogout() {
    signingOut = true;
    try {
      await logout();
    } finally {
      signingOut = false;
    }
  }
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
      {#if $authView.state === "signed_in"}
        <div class="user">
          <span class="user-name">{$authView.session.display_name}</span>
          <span class="user-email faint">{$authView.session.email}</span>
          <button
            class="sign-out"
            onclick={handleLogout}
            type="button"
            disabled={signingOut}
          >
            {signingOut ? "Signing out..." : "Sign out"}
          </button>
        </div>
      {/if}
      <div class="footer-row">
        <button class="theme-toggle" onclick={toggleTheme} type="button">
          {$theme === "dark" ? "Light" : "Dark"} mode
        </button>
        <span class="version faint">v0.1.0</span>
      </div>
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
    flex-direction: column;
    gap: var(--sp-4);
  }
  .footer-row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-3);
  }
  .user {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--sp-3);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
  }
  .user-name {
    font-size: 13px;
    color: var(--text);
    font-weight: 500;
  }
  .user-email {
    font-size: 11px;
    font-family: var(--font-mono);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .sign-out {
    margin-top: var(--sp-2);
    background: transparent;
    border: 0;
    border-top: 1px solid var(--border);
    padding: var(--sp-2) 0 0;
    color: var(--text-muted);
    font-size: 12px;
    text-align: left;
    cursor: pointer;
    transition: color 120ms var(--ease);
  }
  .sign-out:hover {
    color: var(--text);
  }
  .sign-out:disabled {
    opacity: 0.5;
    cursor: progress;
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
