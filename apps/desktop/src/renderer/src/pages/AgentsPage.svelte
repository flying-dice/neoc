<!--
  @page AgentsPage — chat with real ACP agents. Each conversation is an ACP
  session in the main process with the AgentOS MCP tools attached, so agents
  can use installed apps (e.g. put results on the Calculator).
-->
<script lang="ts">
  import { Text } from '@agentos/ui/atoms'
  import { IconButton, ListRow, SearchField } from '@agentos/ui/molecules'
  import { ListSection, NavigationBar } from '@agentos/ui/organisms'
  import { ScreenTemplate, SplitViewTemplate } from '@agentos/ui/templates'
  import MessageBubble from '../atoms/MessageBubble.svelte'
  import RichText from '../atoms/RichText.svelte'
  import StatusDot from '../atoms/StatusDot.svelte'
  import AgentAvatar from '../molecules/AgentAvatar.svelte'
  import PermissionCard from '../molecules/PermissionCard.svelte'
  import TurnActivity from '../molecules/TurnActivity.svelte'
  import Composer from '../organisms/Composer.svelte'
  import SessionBar from '../organisms/SessionBar.svelte'
  import { agentStore, lookOf, type ChatItem } from '../lib/agents.svelte'

  /** Agent-internal plumbing that isn't counted as a user-meaningful tool call. */
  const INTERNAL_TOOLS = /^(ToolSearch|TodoWrite)$/

  type Activity = {
    kind: 'activity'
    id: string
    reasoning: number
    tools: number
    failed: number
    running: boolean
  }
  type Reply = { kind: 'reply'; id: string; text: string }
  type Block = Extract<ChatItem, { kind: 'user' | 'permission' | 'error' }> | Activity | Reply

  /**
   * One turn = the user's message, a single activity line (counts only) and
   * the agent's reply merged into one bubble. Permissions/errors stay in place.
   */
  function toBlocks(items: ChatItem[]): Block[] {
    const blocks: Block[] = []
    let activity: Activity | null = null
    let reply: Reply | null = null
    const ensureActivity = (id: string): Activity => {
      if (!activity) {
        activity = {
          kind: 'activity',
          id: `activity-${id}`,
          reasoning: 0,
          tools: 0,
          failed: 0,
          running: false
        }
        blocks.push(activity)
      }
      return activity
    }
    for (const item of items) {
      switch (item.kind) {
        case 'user':
          blocks.push(item)
          activity = null
          reply = null
          ensureActivity(item.id)
          break
        case 'thought':
          ensureActivity(item.id).reasoning++
          break
        case 'tool': {
          const a = ensureActivity(item.id)
          if (item.status === 'pending' || item.status === 'in_progress') a.running = true
          if (INTERNAL_TOOLS.test(item.title)) break
          a.tools++
          if (item.status === 'failed') a.failed++
          break
        }
        case 'agent':
          if (reply) reply.text += `\n\n${item.text}`
          else blocks.push((reply = { kind: 'reply', id: `reply-${item.id}`, text: item.text }))
          break
        default:
          blocks.push(item)
      }
    }
    return blocks
  }

  const suggestions = [
    'What is 18% of 2,450? Show it on the calculator.',
    'Split a £186.40 bill between 7 people and show me the result.',
    'What calculations have I done recently?'
  ]

  let query = $state('')
  const filtered = $derived(
    agentStore.conversations.filter((c) => c.title.toLowerCase().includes(query.toLowerCase()))
  )
  const convo = $derived(agentStore.selected)
  const busy = $derived(convo?.status === 'running' || convo?.status === 'starting')
  const blocks = $derived(convo ? toBlocks(convo.items) : [])
  /** The newest activity line is live while the agent is busy. */
  const liveActivityId = $derived(
    busy ? blocks.findLast((b) => b.kind === 'activity')?.id : undefined
  )
  const agentName = $derived(
    agentStore.agents.find((a) => a.id === (convo?.agentId ?? 'claude'))?.name ?? 'Claude'
  )

  let thread: HTMLElement | undefined = $state()
  // Keep the newest message in view as the transcript streams.
  $effect(() => {
    void convo?.items.length
    void convo?.items.at(-1)
    queueMicrotask(() => thread?.lastElementChild?.scrollIntoView({ block: 'end' }))
  })

  let draft = $state('')
  let composer: ReturnType<typeof Composer> | undefined = $state()

  function send(text: string): void {
    void agentStore.send(text)
  }
</script>

<SplitViewTemplate sidebarWidth={300}>
  {#snippet sidebar()}
    <NavigationBar testid="agents-sidebar-nav" title="Agents" large>
      {#snippet trailing()}
        <IconButton
          testid="agents-new-chat"
          icon="square.and.pencil"
          label="New chat"
          onclick={() => agentStore.newConversation()}
        />
      {/snippet}
      {#snippet accessory()}
        <SearchField testid="agents-search" bind:value={query} />
      {/snippet}
    </NavigationBar>

    {#if filtered.length}
      <ListSection sidebar>
        {#each filtered as c (c.id)}
          <ListRow
            testid="agents-conversation-{c.id}"
            title={c.title}
            subtitle={lookOf(c.agentId).tagline}
            selected={c.id === agentStore.selectedId}
            onclick={() => (agentStore.selectedId = c.id)}
          >
            {#snippet leading()}
              <AgentAvatar
                icon={lookOf(c.agentId).icon}
                color={lookOf(c.agentId).color}
                size={34}
              />
            {/snippet}
            {#snippet trailing()}
              <StatusDot status={c.status} />
            {/snippet}
          </ListRow>
        {/each}
      </ListSection>
    {:else}
      <div class="sidebar-empty">
        <Text style="footnote" color="secondary">No chats yet.</Text>
      </div>
    {/if}
  {/snippet}

  {#snippet detail()}
    <ScreenTemplate background="plain">
      {#snippet navigationBar()}
        <NavigationBar testid="agents-chat-nav" title={convo?.title ?? agentName} material>
          {#snippet trailing()}
            {#if busy}
              <IconButton
                testid="agents-stop"
                icon="stop"
                label="Stop"
                onclick={() => agentStore.cancel()}
              />
            {/if}
          {/snippet}
        </NavigationBar>
      {/snippet}

      {#snippet content()}
        {#if !agentStore.available}
          <div class="empty">
            <Text style="headline">Agents run in the desktop app</Text>
            <Text style="subheadline" color="secondary"
              >Open AgentOS with <code>bun run dev</code> to chat with a real agent.</Text
            >
          </div>
        {:else if !convo || convo.items.length === 0}
          <div class="empty">
            <AgentAvatar icon={lookOf('claude').icon} color={lookOf('claude').color} size={64} />
            <Text style="title3" weight="semibold">Ask {agentName}</Text>
            <Text style="subheadline" color="secondary">
              {agentName} can use your apps’ tools — try the Calculator.
            </Text>
            <div class="suggestions">
              {#each suggestions as s, i (s)}
                <button
                  type="button"
                  class="suggestion"
                  data-testid="agents-suggestion-{i}"
                  onclick={() => send(s)}
                >
                  {s}
                </button>
              {/each}
            </div>
          </div>
        {:else}
          <div class="thread" bind:this={thread}>
            {#if convo.toolsConnected === false}
              <p class="notice">
                This agent doesn’t support HTTP MCP, so AgentOS tools aren’t available.
              </p>
            {/if}
            {#each blocks as item (item.id)}
              {#if item.kind === 'user'}
                <MessageBubble from="user">{item.text}</MessageBubble>
              {:else if item.kind === 'activity'}
                <TurnActivity
                  reasoning={item.reasoning}
                  tools={item.tools}
                  failed={item.failed}
                  live={item.id === liveActivityId}
                  status={convo.status === 'starting' ? `Starting ${agentName}…` : undefined}
                />
              {:else if item.kind === 'reply'}
                <MessageBubble from="agent"><RichText text={item.text} /></MessageBubble>
              {:else if item.kind === 'permission'}
                <PermissionCard
                  testid="agents-permission-{item.id}"
                  title={item.title}
                  options={item.options}
                  answer={item.answer}
                  onchoose={(optionId) => agentStore.respond(convo, item, optionId)}
                />
              {:else if item.kind === 'error'}
                <p class="notice error">{item.text}</p>
              {/if}
            {/each}
          </div>
        {/if}
      {/snippet}

      {#snippet bottomBar()}
        <Composer
          bind:this={composer}
          bind:value={draft}
          testid="agents-composer"
          placeholder="Message {agentName}"
          disabled={!agentStore.available || busy}
          onsend={send}
        >
          {#snippet accessory()}
            {#if convo}
              <SessionBar
                modes={convo.modes ?? null}
                configOptions={convo.configOptions ?? []}
                commands={convo.commands ?? []}
                usage={convo.usage ?? null}
                disabled={busy || convo.status === 'starting'}
                onmode={(id) => agentStore.setMode(id)}
                onconfig={(id, value) => agentStore.setConfig(id, value)}
                oncommand={(c) => {
                  draft = `/${c.name} `
                  composer?.focus()
                }}
              />
            {/if}
          {/snippet}
        </Composer>
      {/snippet}
    </ScreenTemplate>
  {/snippet}
</SplitViewTemplate>

<style>
  .sidebar-empty {
    padding: 8px 26px;
  }
  .thread {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 0 8px;
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: 56px 24px;
    text-align: center;
  }
  .suggestions {
    display: flex;
    flex-direction: column;
    gap: 8px;
    margin-top: 16px;
    width: min(420px, 100%);
  }
  .suggestion {
    appearance: none;
    border: 0;
    padding: 10px 14px;
    border-radius: 12px;
    background: var(--fill-quaternary);
    color: var(--tint);
    font: var(--type-subheadline);
    text-align: left;
    cursor: pointer;
  }
  .suggestion:hover {
    background: var(--fill-tertiary);
  }
  .notice {
    margin: 4px 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .notice.error {
    color: var(--ios-red);
  }
  code {
    font-family: var(--font-mono);
  }
</style>
