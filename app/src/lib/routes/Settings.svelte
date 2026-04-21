<script lang="ts">
  import { onMount } from "svelte";
  import { theme, toggleTheme } from "../stores/theme";
  import {
    api,
    type UserRecord,
    type RoleRecord,
  } from "../api/tauri";

  let users = $state<UserRecord[]>([]);
  let roles = $state<RoleRecord[]>([]);
  let loading = $state(true);
  let loadErr = $state("");

  // Create user form
  let showUserForm = $state(false);
  let newEmail = $state("");
  let newDisplay = $state("");
  let newPassword = $state("");
  let newRoleId = $state("role-associate");
  let creating = $state(false);
  let createErr = $state("");

  // Change password form
  let oldPassword = $state("");
  let nextPassword = $state("");
  let confirmPassword = $state("");
  let pwSubmitting = $state(false);
  let pwErr = $state("");
  let pwOk = $state("");

  onMount(async () => {
    try {
      const [u, r] = await Promise.all([api.listUsers(), api.listRoles()]);
      users = u;
      roles = r;
      // Default the new-user role picker to the first non-partner role so
      // adding the second user doesn't silently grant full admin.
      const safeDefault = r.find((x) => x.id === "role-associate") ?? r[r.length - 1];
      if (safeDefault) newRoleId = safeDefault.id;
    } catch (e) {
      loadErr = String(e).replace(/^Error:\s*/, "");
    } finally {
      loading = false;
    }
  });

  function openUserForm() {
    showUserForm = true;
    createErr = "";
    newEmail = "";
    newDisplay = "";
    newPassword = "";
  }

  function cancelUserForm() {
    showUserForm = false;
    createErr = "";
  }

  async function submitUser(event: Event) {
    event.preventDefault();
    createErr = "";
    creating = true;
    try {
      const created = await api.createUser({
        email: newEmail,
        display_name: newDisplay,
        password: newPassword,
        role_id: newRoleId,
      });
      users = [...users, created];
      showUserForm = false;
    } catch (e) {
      createErr = String(e).replace(/^Error:\s*/, "");
    } finally {
      creating = false;
    }
  }

  async function submitPassword(event: Event) {
    event.preventDefault();
    pwErr = "";
    pwOk = "";
    if (nextPassword !== confirmPassword) {
      pwErr = "new passwords do not match";
      return;
    }
    pwSubmitting = true;
    try {
      await api.changePassword({
        old_password: oldPassword,
        new_password: nextPassword,
      });
      pwOk = "password updated";
      oldPassword = "";
      nextPassword = "";
      confirmPassword = "";
    } catch (e) {
      pwErr = String(e).replace(/^Error:\s*/, "");
    } finally {
      pwSubmitting = false;
    }
  }

  function fmtDate(ts: number | null) {
    if (ts == null) return "—";
    return new Date(ts * 1000).toLocaleDateString("en-GB", {
      day: "numeric",
      month: "short",
      year: "numeric",
    });
  }
</script>

<header>
  <span class="label">Preferences</span>
  <h1>Settings</h1>
  <p class="muted">Theme, team, security, AI provider, sync behaviour, licence.</p>
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
  <h3>Team</h3>
  <p class="faint">
    Each additional user holds their own wrapped copy of the firm's encryption key, so they can
    open the database with their own password. Only partners can add users.
  </p>

  {#if loading}
    <p class="faint">Loading…</p>
  {:else if loadErr}
    <p class="error">{loadErr}</p>
  {:else}
    <table>
      <thead>
        <tr>
          <th>Name</th>
          <th>Email</th>
          <th>Role</th>
          <th>Last seen</th>
        </tr>
      </thead>
      <tbody>
        {#each users as u (u.id)}
          <tr>
            <td>{u.display_name}</td>
            <td class="muted">{u.email}</td>
            <td class="muted">{u.role_name}</td>
            <td class="faint">{fmtDate(u.last_seen_at)}</td>
          </tr>
        {/each}
      </tbody>
    </table>

    {#if !showUserForm}
      <button class="button" type="button" onclick={openUserForm}>Add user</button>
    {:else}
      <form class="inline-form" onsubmit={submitUser}>
        <div class="field">
          <label for="u-display">Display name</label>
          <input id="u-display" type="text" bind:value={newDisplay} required />
        </div>
        <div class="field">
          <label for="u-email">Email</label>
          <input
            id="u-email"
            type="email"
            bind:value={newEmail}
            autocomplete="off"
            required
          />
        </div>
        <div class="field">
          <label for="u-pw">Initial password</label>
          <input
            id="u-pw"
            type="password"
            bind:value={newPassword}
            autocomplete="new-password"
            minlength="8"
            required
          />
          <span class="faint">The new user can change this after signing in.</span>
        </div>
        <div class="field">
          <label for="u-role">Role</label>
          <select id="u-role" bind:value={newRoleId}>
            {#each roles as r (r.id)}
              <option value={r.id}>{r.name}</option>
            {/each}
          </select>
        </div>

        {#if createErr}<p class="error">{createErr}</p>{/if}

        <div class="row-actions">
          <button type="button" onclick={cancelUserForm} disabled={creating}>Cancel</button>
          <button type="submit" class="primary" disabled={creating}>
            {creating ? "Creating…" : "Create user"}
          </button>
        </div>
      </form>
    {/if}
  {/if}
</section>

<section class="setting-group">
  <h3>Security</h3>
  <p class="faint">
    Change the password that unwraps your copy of the encryption key. Other users are unaffected.
  </p>
  <form class="inline-form" onsubmit={submitPassword}>
    <div class="field">
      <label for="pw-old">Current password</label>
      <input
        id="pw-old"
        type="password"
        bind:value={oldPassword}
        autocomplete="current-password"
        required
      />
    </div>
    <div class="field">
      <label for="pw-new">New password</label>
      <input
        id="pw-new"
        type="password"
        bind:value={nextPassword}
        autocomplete="new-password"
        minlength="8"
        required
      />
    </div>
    <div class="field">
      <label for="pw-new2">Confirm new password</label>
      <input
        id="pw-new2"
        type="password"
        bind:value={confirmPassword}
        autocomplete="new-password"
        minlength="8"
        required
      />
    </div>
    {#if pwErr}<p class="error">{pwErr}</p>{/if}
    {#if pwOk}<p class="ok">{pwOk}</p>{/if}
    <div class="row-actions">
      <button type="submit" class="primary" disabled={pwSubmitting}>
        {pwSubmitting ? "Updating…" : "Update password"}
      </button>
    </div>
  </form>
</section>

<section class="setting-group">
  <h3>AI provider</h3>
  <p class="faint">Subscription, prepaid, or bring-your-own-key. Not implemented in scaffold.</p>
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
    font: inherit;
    margin-top: var(--sp-3);
  }
  .button:hover { border-color: var(--accent); }

  table {
    width: 100%;
    border-collapse: collapse;
    font-size: 13px;
    margin: var(--sp-4) 0;
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

  .inline-form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
    margin-top: var(--sp-4);
    padding: var(--sp-4);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    max-width: 46ch;
  }
  .field { display: flex; flex-direction: column; gap: var(--sp-2); }
  .field label { font-size: 12px; color: var(--text-muted); font-weight: 500; }
  .field input, .field select {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: var(--radius-sm);
  }
  .field input:focus, .field select:focus {
    outline: none;
    border-color: var(--accent);
  }

  .row-actions {
    display: flex;
    gap: var(--sp-3);
    justify-content: flex-end;
  }
  .row-actions button {
    font: inherit;
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    border-radius: var(--radius-sm);
  }
  .row-actions button:disabled { opacity: 0.5; cursor: not-allowed; }
  .primary { border-color: var(--accent) !important; color: var(--accent) !important; }

  .error { color: #b04040; font-size: 13px; margin: 0; }
  .ok { color: #4a8a4a; font-size: 13px; margin: 0; }
</style>
