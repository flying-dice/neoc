<!--
  @template ChatTranscript — a chat with an agent, bound to a `thread` source. The source's declaration names the thread
  and the agent (`params: {thread, agent}`, literals or refs); the messages themselves arrive over `/agui` (HTTP + SSE),
  not over `/rpc`: a `ChatSession` ensures the thread, loads its history and streams runs. The window shows the
  transcript (a live log), the session bar (mode, model, effort, commands, context usage) and the composer. `send` and
  `stop` events are also reported to the app (`{text}` for send) so it can title the thread; the chat works without a
  handler. The component fills the scrolling screen it sits in.
  Organisms: ChatThread, Composer, SessionBar. Atoms: ActivityIndicator, Button, Icon, Text.
  Test ids: the node's test id on the root, `-thread`, `-session`, `-composer`, `-retry` below it.
-->
<script lang="ts">
  import { ActivityIndicator, Button, Icon, Text } from '@agentos/ui/atoms'
  import { ChatThread, Composer, SessionBar } from '@agentos/ui/organisms'
  import { useView } from '../context'
  import { liveActivityId, toBlocks } from '../kit/agui/blocks'
  import { ChatSession, scopedThread, type ChatState } from '../kit/agui/session'
  import { chatOptionsOf } from '../kit/chatOptions'
  import { bool, commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const sourceId = $derived(typeof node.props?.thread === 'string' ? node.props.thread : undefined)
  const decl = $derived(sourceId ? view.sources.declaration(sourceId) : undefined)
  // An app names its threads in its own namespace (`<app>:<id>`): the id comes from the app (often a row of its
  // data), and the renderer, not the app, puts the prefix on, so no app can open or read another app's chat.
  const threadId = $derived(scopedThread(view.app, str(view.evaluate(decl?.params?.thread, scope))))
  const agentId = $derived(str(view.evaluate(decl?.params?.agent, scope)))
  const placeholderProp = $derived(str(p('placeholder')))
  const showComposer = $derived(p('composer') === undefined ? true : bool(p('composer')))

  let session: ChatSession | undefined = $state()
  let chat: ChatState | undefined = $state()
  let draft = $state('')
  let composer: ReturnType<typeof Composer> | undefined = $state()

  // One ChatSession per (thread, agent): a different thread is a different chat, the old stream is dropped.
  $effect(() => {
    if (!threadId || !agentId) return undefined
    const options = chatOptionsOf(ctx.registry)
    const next = new ChatSession({
      threadId,
      agentId,
      fetch: options.fetch,
      baseUrl: options.baseUrl
    })
    session = next
    chat = next.getState()
    draft = ''
    const off = next.subscribe(() => (chat = next.getState()))
    void next.open()
    return () => {
      off()
      next.dispose()
      if (session === next) session = undefined
    }
  })

  const agentName = $derived(chat?.state.session.agent?.name ?? agentId ?? 'the agent')
  const blocks = $derived(chat ? toBlocks(chat.items) : [])
  const live = $derived(chat ? liveActivityId(blocks, chat.status) : undefined)
  const busy = $derived(chat?.status === 'running' || chat?.status === 'connecting')
  const bar = $derived(chat?.state.session)

  function send(text: string): void {
    void session?.send(text)
    view.fire(node, path, 'send', scope, { text })
  }

  function stop(): void {
    void session?.stop()
    view.fire(node, path, 'stop', scope)
  }

  // Fill the scrolling screen: the chat is as tall as what is left below it, and its own transcript scrolls.
  let root: HTMLElement | undefined = $state()
  let height: string | undefined = $state()
  $effect(() => {
    const el = root
    const parent = el?.parentElement
    if (!el || !parent || typeof ResizeObserver === 'undefined') return undefined
    let scroller: HTMLElement | null = null
    for (let n: HTMLElement | null = parent; n; n = n.parentElement) {
      const overflow = getComputedStyle(n).overflowY
      if (overflow === 'auto' || overflow === 'scroll') {
        scroller = n
        break
      }
    }
    if (!scroller) return undefined
    const host = scroller
    const measure = (): void => {
      const top = el.getBoundingClientRect().top - host.getBoundingClientRect().top + host.scrollTop
      const below = parseFloat(getComputedStyle(parent).paddingBottom) || 0
      height = `${Math.max(240, Math.round(host.clientHeight - top - below))}px`
    }
    measure()
    const ro = new ResizeObserver(measure)
    ro.observe(host)
    return () => ro.disconnect()
  })
</script>

{#if !sourceId || !decl}
  <p class="missing" data-testid={common.testid}>This chat has no thread source.</p>
{:else}
  <div class="chat" bind:this={root} style:height data-testid={common.testid} style={common.style}>
    <ChatThread
      testid="{common.testid}-thread"
      {blocks}
      liveActivityId={live}
      busy={chat?.status === 'running'}
      status={chat?.status === 'connecting' ? `Starting ${agentName}…` : undefined}
      onchoose={(id, optionId) => void session?.respond(id, optionId)}
    >
      {#snippet empty()}
        <div class="empty">
          {#if chat?.error}
            <Icon name="exclamationmark.triangle" size={36} />
            <Text style="headline">Could not start {agentName}</Text>
            <p class="error" role="alert">{chat.error}</p>
            <Button
              testid="{common.testid}-retry"
              size="small"
              capsule
              onclick={() => void session?.open()}
            >
              Try again
            </Button>
          {:else if chat?.status === 'connecting' || !chat}
            <ActivityIndicator label="Starting {agentName}" />
            <Text style="subheadline" color="secondary">Starting {agentName}…</Text>
          {:else}
            <Icon name="sparkles" size={40} />
            <Text style="title3" weight="semibold">Ask {agentName}</Text>
            <Text style="subheadline" color="secondary"
              >{agentName} can use the tools of the apps you allow.</Text
            >
          {/if}
        </div>
      {/snippet}
    </ChatThread>

    {#if showComposer}
      <Composer
        bind:this={composer}
        bind:value={draft}
        testid="{common.testid}-composer"
        placeholder={placeholderProp || `Message ${agentName}`}
        disabled={!chat?.ready || chat.status === 'connecting' || chat.status === 'waiting'}
        running={chat?.status === 'running'}
        commands={bar?.commands.map((c) => ({ ...c, hint: c.hint ?? undefined })) ?? []}
        onsend={send}
        onstop={stop}
      >
        {#snippet accessory()}
          {#if bar}
            <SessionBar
              testid="{common.testid}-session"
              modes={bar.modes ?? null}
              settings={bar.config}
              commands={bar.commands}
              usage={bar.usage ?? null}
              disabled={busy}
              onmode={(id) => void session?.setMode(id)}
              onsetting={(id, value) => void session?.setConfig(id, value)}
              oncommand={(c) => {
                draft = `/${c.name} `
                composer?.focus()
              }}
            />
          {/if}
        {/snippet}
      </Composer>
    {/if}
  </div>
{/if}

<style>
  .chat {
    display: flex;
    flex-direction: column;
    min-height: 240px;
    min-width: 0;
    background: var(--bg-elevated);
  }
  .empty {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 8px;
    padding: 48px 24px;
    text-align: center;
    color: var(--label-secondary);
  }
  .error {
    margin: 0;
    max-width: 420px;
    font: var(--type-footnote);
    color: var(--ios-red);
    overflow-wrap: anywhere;
  }
  .missing {
    margin: 12px 16px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
