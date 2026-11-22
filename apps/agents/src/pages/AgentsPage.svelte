<!-- @page AgentsPage — the whole app: the agents and chats beside the open chat, or one of the two in a narrow window; driven by one window's Agents state -->
<script lang="ts">
  import ChatSidebar from '../organisms/ChatSidebar.svelte'
  import ChatThread from '../organisms/ChatThread.svelte'
  import type { Agents } from '../lib/agents.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    agents: Agents
  }

  let { agents }: Props = $props()

  // below 700 wide the list and the chat are one column each: the open chat decides which one shows
  let width = $state(960)
  const narrow = $derived(width < 700)
  const open = $derived(agents.open)

  $effect(() => {
    void agents.load()
    return agents.listen()
  })
</script>

<main class="agents" class:narrow class:chatting={open !== null} bind:clientWidth={width} data-testid="agents-app">
  <div class="list">
    <ChatSidebar
      agents={agents.agents}
      chats={agents.chats}
      selected={open?.chat.id}
      onstart={(agent) => void agents.startChat(agent)}
      onopen={(chat) => void agents.openChat(chat)}
    />
  </div>
  <div class="detail">
    {#if open}
      <ChatThread
        conversation={open}
        draft={agents.draft}
        failure={agents.failure}
        back={narrow}
        ondraft={(text) => (agents.draft = text)}
        onsend={() => void agents.send()}
        onstop={() => void agents.stop()}
        onback={() => agents.back()}
      />
    {:else}
      <div class="nothing">
        {#if agents.failure}
          <ui-banner tone="error" data-testid="agents-failure">{agents.failure}</ui-banner>
        {/if}
        <ui-empty-state
          icon="sparkles"
          heading="No chat open"
          message="Pick an agent in the sidebar to start a chat."
          data-testid="agents-no-chat"
        ></ui-empty-state>
      </div>
    {/if}
  </div>
</main>

<style>
  .agents {
    display: grid;
    grid-template-columns: minmax(220px, 300px) minmax(0, 1fr);
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .list {
    min-width: 0;
    min-height: 0;
    border-inline-end: 1px solid var(--separator);
  }
  .detail {
    min-width: 0;
    min-height: 0;
  }
  .nothing {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 12px 12px;
  }
  /* one column: the list, or the chat once one is open */
  .narrow {
    grid-template-columns: minmax(0, 1fr);
  }
  .narrow.chatting .list,
  .narrow:not(.chatting) .detail {
    display: none;
  }
  .narrow .list {
    border-inline-end: 0;
  }
</style>
