<script lang="ts">
  import Shell from "./lib/components/Shell.svelte";
  import Dashboard from "./lib/routes/Dashboard.svelte";
  import Clients from "./lib/routes/Clients.svelte";
  import Engagements from "./lib/routes/Engagements.svelte";
  import Library from "./lib/routes/Library.svelte";
  import Settings from "./lib/routes/Settings.svelte";
  import { currentRoute } from "./lib/stores/router";
</script>

<Shell>
  {#if $currentRoute === "dashboard"}
    <Dashboard />
  {:else if $currentRoute === "clients"}
    <Clients />
  {:else if $currentRoute === "engagements"}
    <Engagements />
  {:else if $currentRoute === "library"}
    <Library />
  {:else if $currentRoute === "settings"}
    <Settings />
  {/if}
</Shell>
