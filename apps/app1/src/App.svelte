<!-- App 1: a list on the RPC bus. Its bus is the global `RPC` (the host's bridge): nothing to import; `types/` types it. -->
<script lang="ts">
  type Item = VosApp.App1.Item

  let text = $state('')
  let items = $state<Item[]>([])
  let total = $state(0)
  let problem = $state<string | null>(null)
  // a confirmation this window shows for the bus: every open App 1 window shows it, the first answer wins
  let asking = $state<{ question: string; answer: (ok: boolean) => void } | null>(null)

  const load = async (): Promise<void> => {
    items = await RPC.send_request('app1', '_items')
    total = items.length
  }

  // the list now, then every change, whoever made it (this page, another app or an agent)
  $effect(() => {
    void load()
    const stops = [
      RPC.on_request(
        '_client_confirm',
        ({ question }, { signal }) =>
          new Promise<boolean>((resolve) => {
            asking = { question, answer: (ok) => ((asking = null), resolve(ok)) }
            // another window answered first: this one drops the question
            signal.addEventListener('abort', () => (asking = null))
          })
      ),
      RPC.on_notification('app1', 'added', ({ item }) => (items = [...items, item])),
      RPC.on_notification('app1', 'cleared', () => (items = [])),
      // private: only this app hears it
      RPC.on_notification('app1', '_saved', ({ total: now }) => (total = now)),
      vos.host.on('host:reconnected', () => void load())
    ]
    return () => stops.forEach((stop) => stop())
  })

  async function run(action: () => Promise<unknown>): Promise<void> {
    problem = null
    try {
      await action()
    } catch (e) {
      problem = e instanceof Error ? e.message : String(e)
    }
  }

  const add = (): Promise<void> =>
    run(async () => {
      await RPC.send_request('app1', 'add', { text })
      text = ''
    })

  const who = (by: string): string => (by === 'you' ? 'Added by you' : by.startsWith('app:') ? `Added by ${by.slice(4)}` : `Added by ${by.replace(/^agent:/, '')}`)
</script>

<main class="app" data-testid="app1">
  <header>
    <h1>App 1</h1>
    <p data-testid="app1-total">{total} {total === 1 ? 'item' : 'items'}</p>
  </header>

  {#if problem}
    <ui-banner tone="error" data-testid="app1-problem">{problem}</ui-banner>
  {/if}

  <ui-section header="New item" footer="App 2, agents and this page all add through the same request on the bus.">
    <form
      class="composer"
      onsubmit={(e) => {
        e.preventDefault()
        void add()
      }}
    >
      <ui-text-field
        label="Item"
        placeholder="Something to do"
        enterkeyhint="done"
        autocomplete="off"
        hit
        value={text}
        oninput={(e) => (text = (e.currentTarget as unknown as { value: string }).value)}
        data-testid="app1-text"
      ></ui-text-field>
      <ui-button type="submit" variant="filled" hit label="Add" disabled={text.trim() === ''} data-testid="app1-add">Add</ui-button>
    </form>
  </ui-section>

  <ui-section header="Items">
    {#if items.length === 0}
      <ui-empty-state icon="doc" heading="No items" message="Add one here, or from App 2."></ui-empty-state>
    {:else}
      <ui-list variant="insetGrouped" label="Items" data-testid="app1-items">
        {#each items as item (item.id)}
          <ui-row heading={item.text} subheading={who(item.by)} icon="checkmark"></ui-row>
        {/each}
      </ui-list>
      <div class="actions">
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button variant="gray" destructive label="Clear" onclick={() => run(() => RPC.send_request('app1', 'clear'))} data-testid="app1-clear">
          Clear
        </ui-button>
      </div>
    {/if}
  </ui-section>

  <!-- the bus asks this window to confirm: so does every other open App 1 window, and the first answer wins -->
  <ui-alert
    open={asking !== null}
    heading="Clear App 1?"
    message={asking?.question ?? ''}
    action-label="Clear"
    cancel-label="Cancel"
    destructive
    onconfirm={() => asking?.answer(true)}
    oncancel={() => asking?.answer(false)}
    data-testid="app1-confirm"
  ></ui-alert>
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
  .composer {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
  .actions {
    display: flex;
    justify-content: flex-end;
    margin-top: 8px;
  }
</style>
