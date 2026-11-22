<!--
  Agents — the sidebar lists the agents on this host (to start a chat) and the chats, newest first; the detail is the open
  chat. Below 700 px wide they are one column each: opening a chat shows it, Back returns to the list. The list comes
  from the app's backend (`api`); the conversation from the host page's agent chat services (`host.call('vos/agui/...')`).
  Test ids: `agents-app` on the root, `agents-roster` with `agents-agent-<id>`, `agents-chats` with `agents-chat-<id>`,
  `agents-new-chat` (and `agents-new-chat-<id>` in its menu), `agents-back`, `agents-no-chat`, `agents-error` (+ `-retry`),
  and the open chat under `agents-chat`.
-->
<script lang="ts">
  import { api, host, type Agent, type Chat as ChatRow } from 'app:rpc'
  import { hostTransport } from './agui/transport'
  import Chat from './Chat.svelte'
  import { relativeTime } from './time'

  const transport = hostTransport((method, params) => host.call(method, params))

  let roster: Agent[] = $state([])
  let chats: ChatRow[] = $state([])
  let openId: string | null = $state(null)
  let loaded = $state(false)
  let failure: string | null = $state(null)
  let menuOpen = $state(false)
  let compact = $state(false)
  let now = $state(Date.now())

  const current = $derived(chats.find((c) => c.id === openId))
  const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

  /** Bumped by everything the person does: a list that was asked for before that is stale when it arrives. */
  let generation = 0

  async function load(): Promise<void> {
    const asked = generation
    try {
      const [agents, list] = await Promise.all([api.agents(), api.chats()])
      roster = agents
      if (asked !== generation) return
      chats = list.chats
      openId = list.open ?? null
      failure = null
    } catch (e) {
      failure = `Could not load the chats: ${messageOf(e)}`
    } finally {
      loaded = true
    }
  }

  async function act(what: () => Promise<void>): Promise<void> {
    generation++
    try {
      await what()
      failure = null
    } catch (e) {
      failure = messageOf(e)
    }
  }

  const startChat = (agent: Agent): Promise<void> =>
    act(async () => {
      menuOpen = false
      const made = await api.startChat({ agentId: agent.id })
      chats = [made, ...chats.filter((c) => c.id !== made.id)]
      openId = made.id
    })

  const openChat = (id: string | null): Promise<void> =>
    act(async () => {
      openId = id
      const list = await api.openChat({ id })
      chats = list.chats
    })

  function noteMessage(id: string, text: string): void {
    void act(async () => {
      const chat = await api.noteMessage({ id, text })
      chats = [chat, ...chats.filter((c) => c.id !== chat.id)]
    })
  }

  $effect(() => {
    void load()
    // the connection to the host dropped and came back: what changed meanwhile reached nobody
    const off = host.on('host:reconnected', () => void load())
    const tick = setInterval(() => (now = Date.now()), 60_000)
    const narrow = window.matchMedia('(max-width: 699px)')
    const follow = (): void => {
      compact = narrow.matches
    }
    follow()
    narrow.addEventListener('change', follow)
    return () => {
      off()
      clearInterval(tick)
      narrow.removeEventListener('change', follow)
    }
  })

  const showList = $derived(!compact || !current)
  const showDetail = $derived(!compact || !!current)
</script>

<div class="app" class:compact data-testid="agents-app" aria-busy={!loaded}>
  {#if showList}
    <section class="sidebar" aria-label="Agents and chats">
      <header class="bar">
        <h1>Agents</h1>
        {#if roster.length === 1}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
          <ui-button
            data-testid="agents-new-chat"
            variant="plain"
            square
            hit
            label="New chat"
            onclick={() => void startChat(roster[0])}
            ><ui-icon slot="icon" name="square.and.pencil" size={20}></ui-icon></ui-button
          >
        {:else if roster.length > 1}
          <div class="menu-anchor">
            <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
            <ui-button
              data-testid="agents-new-chat"
              variant="plain"
              square
              hit
              label="New chat"
              aria-haspopup="menu"
              aria-expanded={menuOpen}
              onclick={() => (menuOpen = !menuOpen)}
              ><ui-icon slot="icon" name="square.and.pencil" size={20}></ui-icon></ui-button
            >
            {#if menuOpen}
              <div class="menu" role="menu" aria-label="New chat with">
                {#each roster as agent (agent.id)}
                  <button
                    type="button"
                    role="menuitem"
                    data-testid="agents-new-chat-{agent.id}"
                    onclick={() => void startChat(agent)}
                    onkeydown={(e) => {
                      if (e.key === 'Escape') menuOpen = false
                    }}>{agent.name}</button
                  >
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </header>

      {#if failure}
        <ui-banner tone="error" heading="Something went wrong" data-testid="agents-error">
          {failure}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
          <ui-button
            slot="action"
            data-testid="agents-error-retry"
            size="small"
            onclick={() => void load()}>Try again</ui-button
          >
        </ui-banner>
      {/if}

      <div class="lists">
        {#if loaded && roster.length === 0}
          <ui-empty-state
            data-testid="agents-no-agents"
            icon="sparkles"
            heading="No agents"
            message="Add an agent to vos.toml, or with agents.create, to start a chat."
          ></ui-empty-state>
        {:else}
          <!-- two lists: the agents are actions (they start a chat), the chats are a selection -->
          <ui-list variant="sidebar" label="Agents" data-testid="agents-roster">
            <ui-section header="Agents">
              {#each roster as agent (agent.id)}
                <ui-row
                  data-testid="agents-agent-{agent.id}"
                  tappable
                  icon={agent.avatar ?? 'sparkles'}
                  heading={agent.name}
                  subheading={agent.role === '' ? 'New chat' : agent.role}
                  onpress={() => void startChat(agent)}
                ></ui-row>
              {/each}
            </ui-section>
          </ui-list>
        {/if}

        {#if chats.length > 0}
          <ui-list variant="sidebar" selection="single" label="Chats" data-testid="agents-chats">
            <ui-section header="Chats">
              {#each chats as chat (chat.id)}
                <ui-row
                  data-testid="agents-chat-{chat.id}"
                  icon="message"
                  heading={chat.title}
                  subheading="{chat.agentName}  {relativeTime(chat.updatedAt, now)}"
                  selected={chat.id === openId}
                  onpress={() => void openChat(chat.id)}
                ></ui-row>
              {/each}
            </ui-section>
          </ui-list>
        {:else if loaded && roster.length > 0}
          <ui-empty-state
            data-testid="agents-no-chats"
            icon="message"
            heading="No chats yet"
            message="Pick an agent above."
          ></ui-empty-state>
        {/if}
      </div>
    </section>
  {/if}

  {#if showDetail}
    <section class="detail" aria-label={current?.title ?? 'Chat'}>
      <header class="bar">
        {#if compact}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
          <ui-button
            data-testid="agents-back"
            variant="plain"
            hit
            onclick={() => void openChat(null)}
            ><ui-icon slot="icon" name="chevron.left" size={18}></ui-icon>Chats</ui-button
          >
        {/if}
        <h2>{current?.title ?? 'Agents'}</h2>
      </header>
      {#if current}
        {#key current.id}
          <Chat
            testid="agents-chat"
            threadId={current.id}
            agentId={current.agentId}
            agentName={current.agentName}
            {transport}
            onsend={(text) => noteMessage(current.id, text)}
          />
        {/key}
      {:else}
        <ui-empty-state
          data-testid="agents-no-chat"
          icon="message"
          heading="No chat open"
          message="Pick an agent in the sidebar to start a chat."
        ></ui-empty-state>
      {/if}
    </section>
  {/if}
</div>

<style>
  .app {
    box-sizing: border-box;
    display: grid;
    grid-template-columns: minmax(240px, 300px) 1fr;
    height: 100%;
    /* the window's grabber is drawn over the top 28 px of the frame */
    padding-top: 28px;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .app.compact {
    grid-template-columns: 1fr;
  }
  .sidebar {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    background: var(--bg-grouped);
    border-right: var(--hairline) solid var(--separator);
  }
  .compact .sidebar {
    border-right: 0;
  }
  .lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding-bottom: 12px;
  }
  .detail {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    min-height: 44px;
    padding: 0 8px 0 16px;
  }
  .bar h1,
  .bar h2 {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  h1 {
    font: var(--type-title2);
    font-weight: 700;
  }
  h2 {
    font: var(--type-headline);
  }
  .menu-anchor {
    position: relative;
  }
  .menu {
    position: absolute;
    right: 0;
    top: 100%;
    z-index: 2;
    display: flex;
    flex-direction: column;
    min-width: 200px;
    padding: 4px;
    border-radius: 12px;
    background: var(--bg-elevated);
    box-shadow:
      0 0 0 1px var(--separator),
      0 8px 24px rgb(0 0 0 / 0.18);
  }
  .menu button {
    appearance: none;
    border: 0;
    min-height: 44px;
    padding: 0 12px;
    border-radius: 8px;
    background: transparent;
    color: var(--label);
    font: var(--type-body);
    text-align: left;
    cursor: pointer;
  }
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--fill-tertiary);
  }
  ui-empty-state {
    display: block;
    padding: 24px 16px;
  }
</style>
