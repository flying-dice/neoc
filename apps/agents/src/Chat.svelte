<!--
  Chat — one chat with an agent: the transcript (a live log), the session bar (mode, model, effort, commands, context
  usage) and the composer. The conversation reaches the page through the host page's `vos/agui/*` services
  (`agui/transport.ts`); a `ChatSession` ensures the thread, loads its history and pulls each run's events. `onsend` is
  told every message the person sends, so the app can title the chat. The view-tree renderer's `ChatTranscript`, moved here.
  Test ids: `testid` on the root, `-thread`, `-session`, `-composer`, `-retry` below it.
-->
<script lang="ts">
  import { liveActivityId, toBlocks } from './agui/blocks'
  import { ChatSession, type ChatState } from './agui/session'
  import type { ChatTransport } from './agui/transport'
  import ChatThread from './chat/ChatThread.svelte'
  import Composer from './chat/Composer.svelte'
  import SessionBar from './chat/SessionBar.svelte'

  interface Props {
    testid: string
    /** The chat's own id (the host page puts the app in front of it). */
    threadId: string
    agentId: string
    agentName: string
    transport: ChatTransport
    onsend?: (text: string) => void
  }

  let { testid, threadId, agentId, agentName: rosterName, transport, onsend }: Props = $props()

  let session: ChatSession | undefined = $state()
  let chat: ChatState | undefined = $state()
  let draft = $state('')
  let composer: ReturnType<typeof Composer> | undefined = $state()

  // One ChatSession per (thread, agent): a different chat is a different session, and the old one's stream is dropped.
  $effect(() => {
    const next = new ChatSession({ threadId, agentId, transport })
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

  const agentName = $derived(chat?.state.session.agent?.name ?? rosterName)
  const blocks = $derived(chat ? toBlocks(chat.items) : [])
  const live = $derived(chat ? liveActivityId(blocks, chat.status) : undefined)
  const busy = $derived(chat?.status === 'running' || chat?.status === 'connecting')
  const bar = $derived(chat?.state.session)

  function send(text: string): void {
    void session?.send(text)
    onsend?.(text)
  }
</script>

<div class="chat" data-testid={testid}>
  <ChatThread
    testid="{testid}-thread"
    {blocks}
    liveActivityId={live}
    busy={chat?.status === 'running'}
    status={chat?.status === 'connecting' ? `Starting ${agentName}…` : undefined}
    onchoose={(id, optionId) => void session?.respond(id, optionId)}
  >
    {#snippet empty()}
      <div class="empty">
        {#if chat?.error}
          <ui-icon name="exclamationmark.triangle" size={36}></ui-icon>
          <p class="headline">Could not start {agentName}</p>
          <p class="error" role="alert">{chat.error}</p>
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions (a kit button handles its own keys) -->
          <ui-button
            data-testid="{testid}-retry"
            size="small"
            shape="capsule"
            hit
            onclick={() => void session?.open()}>Try again</ui-button
          >
        {:else if chat?.status === 'connecting' || !chat}
          <ui-spinner label="Starting {agentName}"></ui-spinner>
          <p class="note">Starting {agentName}…</p>
        {:else}
          <ui-icon name="sparkles" size={40}></ui-icon>
          <p class="title">Ask {agentName}</p>
          <p class="note">{agentName} can use the tools of the apps you allow.</p>
        {/if}
      </div>
    {/snippet}
  </ChatThread>

  <Composer
    bind:this={composer}
    bind:value={draft}
    testid="{testid}-composer"
    placeholder="Message {agentName}"
    disabled={!chat?.ready || chat.status === 'connecting' || chat.status === 'waiting'}
    running={chat?.status === 'running'}
    commands={bar?.commands.map((c) => ({ ...c, hint: c.hint ?? undefined })) ?? []}
    onsend={send}
    onstop={() => void session?.stop()}
  >
    {#snippet accessory()}
      {#if bar}
        <SessionBar
          testid="{testid}-session"
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
</div>

<style>
  .chat {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
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
  .empty p {
    margin: 0;
  }
  .headline {
    font: var(--type-headline);
    color: var(--label);
  }
  .title {
    font: var(--type-title3);
    font-weight: 600;
    color: var(--label);
  }
  .note {
    font: var(--type-subheadline);
  }
  .error {
    max-width: 420px;
    font: var(--type-footnote);
    color: var(--ios-red);
    overflow-wrap: anywhere;
  }
</style>
