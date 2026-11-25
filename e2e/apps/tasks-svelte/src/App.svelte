<script lang="ts">
  import { api, type Task } from '@tasks-svelte/rpc'
  import { host } from '@vos/host'

  // the journeys reach the backend with calls the screen has no control for (`hang`, `release`)
  ;(window as unknown as { __rpc: (method: string, args?: unknown) => Promise<unknown> }).__rpc = (method, args) =>
    (api as unknown as Record<string, (args?: unknown) => Promise<unknown>>)[method](args)

  let tasks = $state<Task[]>([])
  let title = $state('')
  let error = $state<string | null>(null)
  let theme = $state(host.theme)
  // where the app runs: `host.hosted` is true when the host page draws UI of its own (the shell)
  let where = $state('connecting')

  const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

  // run a method that answers with the whole list; a failure is the backend's own message
  async function show(call: Promise<Task[]>): Promise<void> {
    error = null
    try {
      tasks = await call
    } catch (e) {
      error = messageOf(e)
    }
  }

  // a real form: the frame is sandboxed with `allow-forms`, so `submit` fires (Enter, the button); an unprevented submission would send the
  // frame to another page, so the handler prevents it
  async function add(): Promise<void> {
    await show(api.add({ title }))
    if (!error) title = ''
  }

  // load once, then again whenever the backend says the list changed outside this window (an agent added a task)
  $effect(() => {
    void show(api.list())
    const stops = [
      host.on('tasks.changed', () => void show(api.list())),
      // the host connection came back: a change made while it was away reached nobody
      host.on('host:reconnected', () => void show(api.list())),
      host.onContext((context) => (theme = context.theme === 'dark' ? 'dark' : 'light'))
    ]
    void host.ready.then(() => {
      theme = host.theme
      where = host.hosted ? 'in the shell' : 'on its own'
    })
    return () => stops.forEach((stop) => stop())
  })
</script>

<main class="app" data-testid="tasks-svelte-app">
  <h1>Tasks</h1>
  <p class="meta">
    <span data-testid="tasks-svelte-framework">Svelte</span> ·
    <span data-testid="tasks-svelte-theme">{theme}</span> ·
    <span data-testid="tasks-svelte-where">{where}</span> ·
    <span data-testid="tasks-svelte-count">{tasks.filter((t) => !t.done).length} left</span>
  </p>
  <form
    class="row"
    onsubmit={(e) => {
      e.preventDefault()
      void add()
    }}
  >
    <input
      bind:value={title}
      aria-label="New task"
      placeholder="What has to be done?"
      data-testid="tasks-svelte-title"
    />
    <button type="submit" data-testid="tasks-svelte-add">Add</button>
  </form>
  {#if error}
    <p class="error" role="alert" data-testid="tasks-svelte-error">{error}</p>
  {/if}
  <ul>
    {#each tasks as task (task.id)}
      <li data-testid="tasks-svelte-item-{task.id}" class:done={task.done}>
        <label>
          <input
            type="checkbox"
            checked={task.done}
            onchange={() => show(api.toggle({ id: task.id }))}
            data-testid="tasks-svelte-toggle-{task.id}"
          />
          <span>{task.title}</span>
        </label>
      </li>
    {/each}
  </ul>
</main>

<style>
  :global(html) {
    --bg: #ffffff;
    --fg: #1c1c1e;
    --muted: #5c5c61;
    --line: #c7c7cc;
    --accent: #0a60d6;
    --danger: #b3261e;
  }
  :global(html[data-theme='dark']) {
    --bg: #1c1c1e;
    --fg: #f2f2f7;
    --muted: #aeaeb2;
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
    margin: 0;
    font-size: 22px;
  }
  .meta {
    margin: 4px 0 12px;
    color: var(--muted);
  }
  .row {
    display: flex;
    gap: 8px;
  }
  input:not([type]) {
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
  li label {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    border-bottom: 1px solid var(--line);
  }
  li input[type='checkbox'] {
    width: 22px;
    height: 22px;
  }
  .done span {
    color: var(--muted);
    text-decoration: line-through;
  }
  .error {
    margin: 8px 0 0;
    color: var(--danger);
  }
</style>
