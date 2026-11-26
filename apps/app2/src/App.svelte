<!-- App 2: talks to App 1 on the RPC bus. Its bus is the global `RPC` (the host's bridge): nothing to import; `types/` types it. -->
<script lang="ts">
  type Line = { at: string; text: string }
  let text = $state('')
  let page = $state<Line[]>([])
  let backend = $state<string[]>([])
  let problem = $state<string | null>(null)

  const now = (): string => new Date().toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
  const say = (line: string): void => {
    page = [{ at: now(), text: line }, ...page].slice(0, 50)
  }

  // this page hears App 1 itself (page to app), and hears what App 2's backend heard (app to app, then to this page)
  $effect(() => {
    void RPC.send_request('app2', '_heard').then((lines) => (backend = lines))
    const stops = [
      RPC.on_notification('app1', 'added', ({ item }) => say(`Added “${item.text}” (${item.by})`)),
      RPC.on_notification('app1', 'cleared', ({ count }) => say(`Cleared ${count} item${count === 1 ? '' : 's'}`)),
      RPC.on_notification('app2', '_overheard', ({ line }) => (backend = [line, ...backend].slice(0, 50)))
    ]
    return () => stops.forEach((stop) => stop())
  })

  async function run(action: () => Promise<unknown>): Promise<void> {
    problem = null
    try {
      await action()
      text = ''
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e)
    }
  }
</script>

<main class="app" data-testid="app2">
  <header>
    <h1>App 2</h1>
    <p>Talks to App 1 on the bus</p>
  </header>

  {#if problem}
    <ui-banner tone="error">{problem}</ui-banner>
  {/if}

  <ui-section header="Send to App 1" footer="From this page the request goes straight onto the bus; from the backend it goes through App 2's own handler first.">
    <ui-text-field
      label="Item for App 1"
      placeholder="Something for App 1"
      autocomplete="off"
      hit
      value={text}
      oninput={(e) => (text = (e.currentTarget as unknown as { value: string }).value)}
      data-testid="app2-text"
    ></ui-text-field>
    <div class="buttons">
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="filled"
        hit
        label="Add from this page"
        disabled={text.trim() === ''}
        onclick={() => run(() => RPC.send_request('app1', 'add', { text }))}
        data-testid="app2-add-page">From this page</ui-button
      >
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="tinted"
        hit
        label="Add from App 2's backend"
        disabled={text.trim() === ''}
        onclick={() => run(() => RPC.send_request('app2', '_relay', { text }))}
        data-testid="app2-add-backend">From the backend</ui-button
      >
    </div>
  </ui-section>

  <ui-section header="This page heard">
    {#if page.length === 0}
      <ui-empty-state icon="bell" heading="Nothing yet" message="Add an item here, or in App 1."></ui-empty-state>
    {:else}
      <ui-list variant="insetGrouped" label="What this page heard" data-testid="app2-page-log">
        {#each page as line, i (i)}
          <ui-row heading={line.text} subheading={line.at} icon="bell"></ui-row>
        {/each}
      </ui-list>
    {/if}
  </ui-section>

  <ui-section header="App 2's backend heard">
    {#if backend.length === 0}
      <ui-empty-state icon="cpu" heading="Nothing yet" message="App 2's backend listens to App 1 too."></ui-empty-state>
    {:else}
      <ui-list variant="insetGrouped" label="What App 2's backend heard" data-testid="app2-backend-log">
        {#each backend as line, i (i)}
          <ui-row heading={line} icon="cpu"></ui-row>
        {/each}
      </ui-list>
    {/if}
  </ui-section>
</main>

<style>
  .app {
    box-sizing: border-box;
    min-height: 100%;
    padding: 16px 20px 24px;
    display: flex;
    flex-direction: column;
    gap: 12px;
    background: var(--bg-grouped);
    color: var(--label);
    font: var(--type-body);
  }
  header h1 {
    margin: 0;
    font: var(--type-large-title);
  }
  header p {
    margin: 2px 0 0;
    color: var(--label-secondary);
    font: var(--type-subheadline);
  }
  .buttons {
    display: flex;
    gap: 8px;
    margin-top: 8px;
  }
  .buttons ui-button {
    flex: 1;
  }
</style>
