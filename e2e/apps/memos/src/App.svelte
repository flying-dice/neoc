<script lang="ts">
  type Note = VosApp.Memos.Note

  // the page's methods are the app's private ones on the bus
  const api = {
    list: (): Promise<Note[]> => RPC.send_request('memos', '_list'),
    add: (params: VosApp.Memos.AddParams): Promise<Note[]> => RPC.send_request('memos', '_add', params)
  }

  let notes = $state<Note[]>([])
  let body = $state('')
  let error = $state<string | null>(null)

  // run a method that answers with the whole list; a failure is the backend's own message
  async function show(call: Promise<Note[]>): Promise<void> {
    error = null
    try {
      notes = await call
    } catch (e) {
      error = e instanceof Error ? e.message : String(e)
    }
  }

  async function add(): Promise<void> {
    await show(api.add({ body }))
    if (!error) body = ''
  }

  $effect(() => {
    void show(api.list())
  })
</script>

<main class="app" data-testid="memos-app">
  <h1>Memos</h1>
  <form
    class="row"
    onsubmit={(e) => {
      e.preventDefault()
      void add()
    }}
  >
    <input bind:value={body} aria-label="New note" placeholder="Start writing…" data-testid="memos-body" />
    <button type="submit" data-testid="memos-add">Add</button>
  </form>
  {#if error}
    <p class="error" role="alert" data-testid="memos-error">{error}</p>
  {/if}
  <ul>
    {#each notes as note (note.id)}
      <li data-testid="memos-item-{note.id}">{note.body}</li>
    {/each}
  </ul>
</main>

<style>
  :global(html) {
    --bg: #ffffff;
    --fg: #1c1c1e;
    --line: #c7c7cc;
    --accent: #0a60d6;
    --danger: #b3261e;
  }
  :global(html[data-theme='dark']) {
    --bg: #1c1c1e;
    --fg: #f2f2f7;
    --line: #48484a;
    --accent: #64a8ff;
    --danger: #ff8a80;
  }
  :global(body) {
    background: var(--bg);
    color: var(--fg);
    font: 16px/1.4 system-ui, sans-serif;
  }
  .app {
    padding: 16px;
  }
  h1 {
    margin: 0 0 12px;
    font-size: 22px;
  }
  .row {
    display: flex;
    gap: 8px;
  }
  input {
    flex: 1;
    min-width: 0;
    min-height: 44px;
    padding: 0 10px;
    background: var(--bg);
    color: var(--fg);
    border: 1px solid var(--line);
    border-radius: 8px;
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
  ul {
    margin: 12px 0 0;
    padding: 0;
    list-style: none;
  }
  li {
    padding: 10px 0;
    border-bottom: 1px solid var(--line);
  }
  .error {
    margin: 8px 0 0;
    color: var(--danger);
  }
</style>
