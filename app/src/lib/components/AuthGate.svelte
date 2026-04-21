<script lang="ts">
  import type { Snippet } from "svelte";
  import { onMount } from "svelte";
  import { authView, refreshAuth } from "../stores/auth";
  import Onboarding from "../routes/Onboarding.svelte";
  import SignIn from "../routes/SignIn.svelte";

  type Props = { children: Snippet };
  let { children }: Props = $props();

  let err = $state("");

  onMount(async () => {
    try {
      await refreshAuth();
    } catch (e) {
      err = String(e);
    }
  });
</script>

{#if $authView.state === "loading"}
  <div class="auth-loading">
    {#if err}
      <p class="error">Could not reach the backend: {err}</p>
    {:else}
      <span class="faint">Loading...</span>
    {/if}
  </div>
{:else if $authView.state === "onboarding"}
  <Onboarding />
{:else if $authView.state === "sign_in"}
  <SignIn />
{:else if $authView.state === "signed_in"}
  {@render children()}
{/if}

<style>
  .auth-loading {
    min-height: 100%;
    display: grid;
    place-items: center;
    background: var(--bg);
    color: var(--text-faint);
  }
  .error {
    color: var(--err);
    font-size: 13px;
    max-width: 52ch;
    text-align: center;
  }
</style>
