<script lang="ts">
  import { api, type Count } from '@ticker/rpc'
  import { host } from '@vos/host'

  let count = $state<Count | null>(null)
  let notice = $state<string | null>(null)

  const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

  // run a control: it answers with the count, and a failure is the backend's own message
  async function show(call: Promise<Count | string>): Promise<void> {
    notice = null
    try {
      const answer = await call
      if (typeof answer !== 'string') count = answer
    } catch (e) {
      notice = messageOf(e)
    }
  }

  // read once, then follow the backend: its task tells every open window of each tick
  $effect(() => {
    void show(api.read())
    const stops = [
      host.on('count', (data) => (count = data as Count)),
      // the host connection came back: the ticks made while it was away reached nobody
      host.on('host:reconnected', () => void show(api.read()))
    ]
    return () => stops.forEach((stop) => stop())
  })
</script>

<main class="app" data-testid="ticker-app">
  {#if count}
    <p class="ticks" aria-label="Ticks" data-testid="ticker-ticks">{count.ticks}</p>
    <p class="status" data-testid="ticker-status">{count.running ? 'Counting' : 'Paused'}</p>
    <div class="row">
      <button type="button" onclick={() => show(api.toggle())} data-testid="ticker-toggle">
        {count.running ? 'Pause' : 'Resume'}
      </button>
      <button type="button" onclick={() => show(api.reset())} data-testid="ticker-reset">Reset</button>
      <button type="button" class="destructive" onclick={() => show(api.fail())} data-testid="ticker-fail">Fail</button>
    </div>
  {/if}
  {#if notice}
    <p class="notice" role="alert" data-testid="ticker-notice">{notice}</p>
  {/if}
</main>

<style>
  :global(html) {
    --bg: #ffffff;
    --fg: #1c1c1e;
    --muted: #5c5c61;
    --accent: #0a60d6;
    --danger: #b3261e;
  }
  :global(html[data-theme='dark']) {
    --bg: #1c1c1e;
    --fg: #f2f2f7;
    --muted: #aeaeb2;
    --accent: #64a8ff;
    --danger: #ff8a80;
  }
  :global(body) {
    background: var(--bg);
    color: var(--fg);
    font: 16px/1.4 system-ui, sans-serif;
  }
  .app {
    box-sizing: border-box;
    height: 100%;
    padding: 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 8px;
  }
  p {
    margin: 0;
  }
  .ticks {
    font-size: 48px;
    font-weight: 700;
    font-variant-numeric: tabular-nums;
  }
  .status {
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: 8px;
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
  .destructive {
    background: var(--danger);
  }
  .notice {
    color: var(--danger);
  }
</style>
