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

  // in a window too narrow for both, the split view shows one column: the kit says when, the same for every app
  let narrow = $state(false)
  const open = $derived(agents.open)

  $effect(() => {
    void agents.load()
    return agents.listen()
  })
</script>

<main class="agents" data-testid="agents-app">
  <!-- one column in a narrow window: the open chat is in front of the list -->
  <ui-split-view column={open === null ? 'sidebar' : 'detail'} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <ChatSidebar
        agents={agents.agents}
        chats={agents.chats}
        selected={open?.chat.id}
        onstart={(agent) => void agents.startChat(agent)}
        onopen={(chat) => void agents.openChat(chat)}
      />
    </div>
    <div>
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
  </ui-split-view>
</main>

<style>
  .agents {
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  .nothing {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 12px 12px;
  }
</style>
