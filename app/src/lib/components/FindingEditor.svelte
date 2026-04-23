<script lang="ts">
  import { untrack } from "svelte";
  import {
    api,
    type FindingSummary,
    type SeveritySummary,
  } from "../api/tauri";

  interface Props {
    finding: FindingSummary;
    severities: SeveritySummary[];
    onSaved: (updated: FindingSummary) => void;
    onCancel: () => void;
  }

  let { finding, severities, onSaved, onCancel }: Props = $props();

  // Callers mount a fresh editor per finding (via `{#if editingFindingId ===
  // f.id}`), so seeding local state from the initial prop is intentional —
  // `untrack` tells Svelte we only want the first read.
  let title = $state(untrack(() => finding.title));
  let condition = $state(untrack(() => finding.condition_text ?? ""));
  let criteria = $state(untrack(() => finding.criteria_text ?? ""));
  let cause = $state(untrack(() => finding.cause_text ?? ""));
  let effect = $state(untrack(() => finding.effect_text ?? ""));
  let recommendation = $state(untrack(() => finding.recommendation_text ?? ""));
  let severityId = $state(
    untrack(() => finding.severity_id ?? severities[0]?.id ?? ""),
  );
  let saving = $state(false);
  let err = $state("");

  async function save(event: Event) {
    event.preventDefault();
    err = "";
    saving = true;
    try {
      const updated = await api.engagementUpdateFinding({
        finding_id: finding.id,
        title,
        condition_text: condition,
        criteria_text: criteria,
        cause_text: cause,
        effect_text: effect,
        recommendation_text: recommendation,
        severity_id: severityId,
      });
      onSaved(updated);
    } catch (e) {
      err = String(e);
    } finally {
      saving = false;
    }
  }
</script>

<form class="form" onsubmit={save}>
  <div class="form-head">
    <h3>Edit {finding.code}</h3>
    <span class="faint small">
      Condition · Criteria · Cause · Effect · Recommendation
    </span>
  </div>

  <label>
    <span class="label">Title</span>
    <input type="text" bind:value={title} required />
  </label>

  <label>
    <span class="label">Severity</span>
    <select bind:value={severityId} required>
      {#each severities as s (s.id)}
        <option value={s.id}>{s.name}</option>
      {/each}
    </select>
  </label>

  <label>
    <span class="label">Condition</span>
    <textarea rows="3" bind:value={condition}></textarea>
    <span class="faint hint">
      What testing found. Facts, specifics, sample sizes — not opinion.
    </span>
  </label>

  <label>
    <span class="label">Criteria</span>
    <textarea rows="3" bind:value={criteria}></textarea>
    <span class="faint hint">
      The standard, policy, or control objective the condition is measured
      against. For example, "Firm joiner/leaver policy §4.2 requires access
      disabled within 24 hours of termination".
    </span>
  </label>

  <label>
    <span class="label">Cause</span>
    <textarea rows="3" bind:value={cause}></textarea>
    <span class="faint hint">
      Why the condition occurred. Root cause, not a symptom. Informs
      recommendation — fix the cause, not the instance.
    </span>
  </label>

  <label>
    <span class="label">Effect</span>
    <textarea rows="3" bind:value={effect}></textarea>
    <span class="faint hint">
      Impact or risk arising from the condition. Be concrete — link to
      financial, regulatory, or operational exposure.
    </span>
  </label>

  <label>
    <span class="label">Recommendation</span>
    <textarea rows="3" bind:value={recommendation}></textarea>
    <span class="faint hint">
      Practical remediation the auditor is asking management to do. Addresses
      the cause, not just the visible exception.
    </span>
  </label>

  {#if err}
    <p class="form-err">{err}</p>
  {/if}

  <div class="form-actions">
    <button type="button" onclick={onCancel} disabled={saving}>Cancel</button>
    <button type="submit" class="primary" disabled={saving}>
      {saving ? "Saving…" : "Save"}
    </button>
  </div>
</form>

<style>
  .form {
    display: flex;
    flex-direction: column;
    gap: var(--sp-4);
  }
  .form-head {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: var(--sp-4);
  }
  .form-head h3 { margin: 0; }
  .form label {
    display: flex;
    flex-direction: column;
    gap: var(--sp-2);
  }
  .form input,
  .form select {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
  }
  .form textarea {
    font: inherit;
    padding: var(--sp-2) var(--sp-3);
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    border-radius: 2px;
    resize: vertical;
    min-height: 64px;
  }
  .form input:focus,
  .form select:focus,
  .form textarea:focus {
    outline: none;
    border-color: var(--accent);
  }

  button {
    font: inherit;
    padding: var(--sp-2) var(--sp-4);
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
    border-radius: 2px;
  }
  button:hover:not(:disabled) { background: var(--accent-soft); }
  button:disabled { opacity: 0.5; cursor: not-allowed; }
  button.primary {
    border-color: var(--accent);
    color: var(--accent);
  }

  .form-actions {
    display: flex;
    gap: var(--sp-3);
    justify-content: flex-end;
    margin-top: var(--sp-2);
  }
  .form-err {
    color: #b04040;
    font-size: 13px;
    margin: 0;
  }
  .hint { font-size: 12px; }
  .small { font-size: 11px; }
</style>
