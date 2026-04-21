<script lang="ts">
  import { theme, toggleTheme } from "../stores/theme";
</script>

<header>
  <span class="label">Preferences</span>
  <h1>Settings</h1>
  <p class="muted">Theme, firm profile, AI provider, sync behaviour, licence.</p>
</header>

<hr />

<section class="setting-group">
  <h3>Appearance</h3>
  <div class="row">
    <div>
      <p>Theme</p>
      <p class="faint">Matches system preference on first launch, then persists your choice.</p>
    </div>
    <button class="button" onclick={toggleTheme} type="button">
      Switch to {$theme === "dark" ? "light" : "dark"}
    </button>
  </div>
</section>

<section class="setting-group">
  <h3>Firm</h3>
  <p class="faint">Firm profile, team roster, licence tier — configured in the first-run onboarding flow.</p>
</section>

<section class="setting-group">
  <h3>AI provider</h3>
  <p class="faint">Subscription, prepaid, or bring-your-own-key. Fallback provider list. Not implemented in scaffold.</p>
</section>

<section class="setting-group">
  <h3>Sync</h3>
  <p class="faint">Local-only, encrypted sync to hosted service, or custom endpoint. Not implemented in scaffold.</p>
</section>

<style>
  header { margin-bottom: var(--sp-5); }
  header h1 { margin-top: var(--sp-2); }
  header p { margin-top: var(--sp-3); max-width: 62ch; }

  .setting-group {
    max-width: 62ch;
    padding: var(--sp-5) 0;
    border-bottom: 1px solid var(--border);
  }
  .setting-group:last-of-type { border-bottom: 0; }
  .setting-group h3 { margin-bottom: var(--sp-3); }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--sp-4);
  }
  .button {
    background: var(--surface-elevated);
    border: 1px solid var(--border-strong);
    border-radius: var(--radius-sm);
    padding: var(--sp-2) var(--sp-4);
    color: var(--text);
    cursor: pointer;
    transition: border-color 120ms var(--ease);
  }
  .button:hover { border-color: var(--accent); }
</style>
