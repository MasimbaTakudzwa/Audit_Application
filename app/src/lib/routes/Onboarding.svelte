<script lang="ts">
  import { onboard } from "../stores/auth";

  let firmName = $state("");
  let firmCountry = $state("Zimbabwe");
  let displayName = $state("");
  let email = $state("");
  let password = $state("");

  let submitting = $state(false);
  let err = $state("");

  async function submit(event: SubmitEvent) {
    event.preventDefault();
    err = "";
    submitting = true;
    try {
      await onboard({
        firm_name: firmName,
        firm_country: firmCountry,
        display_name: displayName,
        email,
        password,
      });
    } catch (e) {
      err = String(e).replace(/^Error:\s*/, "");
      submitting = false;
    }
  }
</script>

<div class="auth-screen">
  <section class="card auth-card">
    <header>
      <span class="label">First run</span>
      <h1>Set up your firm</h1>
      <p class="muted">
        Create the audit workspace. The account you create now becomes the firm's
        first partner; additional users can be added from the settings later.
      </p>
    </header>

    <form onsubmit={submit} novalidate>
      <div class="field">
        <label for="firm-name">Firm name</label>
        <input
          id="firm-name"
          type="text"
          bind:value={firmName}
          autocomplete="organization"
          required
        />
      </div>

      <div class="field">
        <label for="firm-country">Country</label>
        <input
          id="firm-country"
          type="text"
          bind:value={firmCountry}
          autocomplete="country-name"
          required
        />
      </div>

      <hr />

      <div class="field">
        <label for="display-name">Your name</label>
        <input
          id="display-name"
          type="text"
          bind:value={displayName}
          autocomplete="name"
          required
        />
      </div>

      <div class="field">
        <label for="email">Email</label>
        <input
          id="email"
          type="email"
          bind:value={email}
          autocomplete="email"
          required
        />
      </div>

      <div class="field">
        <label for="password">Password</label>
        <input
          id="password"
          type="password"
          bind:value={password}
          autocomplete="new-password"
          minlength="8"
          required
        />
        <span class="hint faint">At least 8 characters. Stored as an Argon2id hash.</span>
      </div>

      {#if err}
        <p class="error">{err}</p>
      {/if}

      <button type="submit" class="primary" disabled={submitting}>
        {submitting ? "Creating..." : "Begin"}
      </button>
    </form>
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
    max-width: 460px;
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
  .hint {
    font-size: 12px;
  }
  hr {
    margin: var(--sp-2) 0;
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
  .primary:hover {
    opacity: 0.88;
  }
  .primary:disabled {
    opacity: 0.5;
    cursor: progress;
  }

  .error {
    color: var(--err);
    font-size: 13px;
  }
</style>
