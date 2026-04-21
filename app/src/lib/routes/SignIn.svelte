<script lang="ts">
  import { login, resetIdentity } from "../stores/auth";

  const CONFIRM_PHRASE = "i understand this wipes everything";

  let email = $state("");
  let password = $state("");
  let submitting = $state(false);
  let err = $state("");

  let showReset = $state(false);
  let resetConfirm = $state("");
  let resetting = $state(false);
  let resetErr = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    err = "";
    submitting = true;
    try {
      await login({ email, password });
    } catch (e) {
      err = String(e).replace(/^Error:\s*/, "");
      submitting = false;
    }
  }

  function openReset() {
    showReset = true;
    resetErr = "";
    resetConfirm = "";
  }

  function cancelReset() {
    showReset = false;
    resetErr = "";
    resetConfirm = "";
  }

  async function submitReset(event: SubmitEvent) {
    event.preventDefault();
    resetErr = "";
    resetting = true;
    try {
      await resetIdentity(resetConfirm);
    } catch (e) {
      resetErr = String(e).replace(/^Error:\s*/, "");
      resetting = false;
    }
  }
</script>

<div class="auth-screen">
  <section class="card auth-card">
    {#if !showReset}
      <header>
        <span class="label">Sign in</span>
        <h1>Welcome back</h1>
        <p class="muted">
          Enter the credentials you set up when you created the firm.
        </p>
      </header>

      <form onsubmit={submit} novalidate>
        <div class="field">
          <label for="email">Email</label>
          <!-- svelte-ignore a11y_autofocus -->
          <input
            id="email"
            type="email"
            bind:value={email}
            autocomplete="email"
            autofocus
            required
          />
        </div>

        <div class="field">
          <label for="password">Password</label>
          <input
            id="password"
            type="password"
            bind:value={password}
            autocomplete="current-password"
            required
          />
        </div>

        {#if err}
          <p class="error">{err}</p>
        {/if}

        <button type="submit" class="primary" disabled={submitting}>
          {submitting ? "Signing in..." : "Continue"}
        </button>

        <button type="button" class="linklike" onclick={openReset}>
          Forgot password?
        </button>
      </form>
    {:else}
      <header>
        <span class="label danger">Reset</span>
        <h1>Start over?</h1>
        <p class="muted">
          Because the password derives the key that encrypts your data, there is no
          way to recover it. A reset deletes your local firm, clients, engagements, and
          identity file, and takes you back to the first-run setup screen.
        </p>
        <p class="muted">
          To confirm, type the phrase below exactly.
        </p>
      </header>

      <form onsubmit={submitReset} novalidate>
        <div class="field">
          <label for="confirm">Confirmation phrase</label>
          <input
            id="confirm"
            type="text"
            bind:value={resetConfirm}
            autocomplete="off"
            placeholder={CONFIRM_PHRASE}
            required
          />
          <span class="faint">Type: <code>{CONFIRM_PHRASE}</code></span>
        </div>

        {#if resetErr}
          <p class="error">{resetErr}</p>
        {/if}

        <div class="row-actions">
          <button type="button" onclick={cancelReset} disabled={resetting}>
            Cancel
          </button>
          <button type="submit" class="danger-btn" disabled={resetting}>
            {resetting ? "Wiping..." : "Reset and wipe"}
          </button>
        </div>
      </form>
    {/if}
  </section>
</div>

<style>
  .auth-screen {
    min-height: 100%;
    display: grid;
    place-items: center;
    padding: var(--sp-7);
    background: var(--bg);
  }
  .auth-card {
    width: 100%;
    max-width: 440px;
    padding: var(--sp-7);
  }
  header {
    margin-bottom: var(--sp-5);
  }
  header h1 {
    margin-top: var(--sp-2);
  }
  header p {
    margin-top: var(--sp-3);
    max-width: 46ch;
  }
  .label.danger { color: #b04040; }

  form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }
  .field {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  label {
    font-size: 12px;
    color: var(--text-muted);
    font-weight: 500;
  }
  input {
    background: var(--bg);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius-sm);
    padding: var(--sp-3);
    font-family: var(--font-sans);
    font-size: 14px;
    transition: border-color 120ms var(--ease);
  }
  input:focus {
    outline: none;
    border-color: var(--accent);
  }
  code {
    background: var(--surface-sunken, var(--bg));
    padding: 1px 6px;
    border-radius: 3px;
    font-size: 12px;
  }

  .primary {
    margin-top: var(--sp-3);
    background: var(--text);
    color: var(--bg);
    border: 0;
    border-radius: var(--radius-sm);
    padding: var(--sp-3) var(--sp-5);
    cursor: pointer;
    font-weight: 500;
    transition: opacity 120ms var(--ease);
  }
  .primary:hover { opacity: 0.88; }
  .primary:disabled { opacity: 0.5; cursor: progress; }

  .linklike {
    background: transparent;
    border: 0;
    color: var(--text-muted);
    font-size: 13px;
    padding: 0;
    cursor: pointer;
    align-self: center;
    text-decoration: underline;
  }
  .linklike:hover { color: var(--accent); }

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
  .danger-btn {
    border-color: #b04040 !important;
    color: #b04040 !important;
  }
  .danger-btn:hover { background: rgba(176, 64, 64, 0.08); }

  .error {
    color: #b04040;
    font-size: 13px;
  }
</style>
