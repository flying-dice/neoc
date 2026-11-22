<script lang="ts">
  import { api, host, type NotifyParams } from 'app:rpc'

  let sent = $state(0)

  const BUTTONS: { kind: NotifyParams['kind']; title: string }[] = [
    { kind: 'later', title: 'Notify in a moment' },
    { kind: 'burst', title: 'Notify four times' },
    { kind: 'long', title: 'Notify with long text' },
    { kind: 'odd', title: 'Notify with odd text' }
  ]

  // read once, then follow the backend: its background work says how many it has raised
  $effect(() => {
    const read = (): Promise<void> => api.sent().then((count) => void (sent = count))
    void read()
    const stops = [host.on('sent', (data) => (sent = data as number)), host.on('host:reconnected', () => void read())]
    return () => stops.forEach((stop) => stop())
  })
</script>

<main class="app" data-testid="notifier-app">
  <p class="sent" aria-label="Sent" data-testid="notifier-sent">{sent}</p>
  {#each BUTTONS as button (button.kind)}
    <button type="button" onclick={() => api.notify({ kind: button.kind })} data-testid="notifier-{button.kind}">
      {button.title}
    </button>
  {/each}
</main>

<style>
  :global(html) {
    --bg: #ffffff;
    --fg: #1c1c1e;
    --accent: #0a60d6;
  }
  :global(html[data-theme='dark']) {
    --bg: #1c1c1e;
    --fg: #f2f2f7;
    --accent: #64a8ff;
  }
  :global(body) {
    background: var(--bg);
    color: var(--fg);
    font: 16px/1.4 system-ui, sans-serif;
  }
  .app {
    box-sizing: border-box;
    min-height: 100%;
    padding: 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  .sent {
    margin: 0;
    font-size: 40px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  button {
    min-height: 44px;
    padding: 0 16px;
    background: var(--accent);
    color: var(--bg);
    border: 0;
    border-radius: 8px;
    font: inherit;
    font-weight: 600;
  }
</style>
