<!-- A stand-in page while the Copilot app's own is made: it lists the copilots the backend has. -->
<script lang="ts">
  let names = $state<string[]>([])
  let problem = $state<string | null>(null)

  $effect(() => {
    RPC.send_request('copilot', '_copilots').then(
      (copilots) => (names = copilots.map((copilot) => copilot.name)),
      (e: unknown) => (problem = e instanceof Error ? e.message : String(e))
    )
  })
</script>

<main data-testid="copilot">
  <h1>Copilot</h1>
  {#if problem}
    <ui-banner tone="error">{problem}</ui-banner>
  {:else if names.length === 0}
    <p>No copilots yet.</p>
  {:else}
    <ul>
      {#each names as name, i (i)}
        <li>{name}</li>
      {/each}
    </ul>
  {/if}
</main>

<style>
  main {
    padding: 16px;
  }
</style>
