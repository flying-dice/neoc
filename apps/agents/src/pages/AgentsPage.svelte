<!-- @page AgentsPage — the whole app: the agents and chats beside the open chat, or one of the two in a narrow window; driven by one window's Agents state -->
<script lang="ts">
  import AgentForm from '../organisms/AgentForm.svelte'
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
  <!-- one column in a narrow window: the open chat, or the agent form, is in front of the list -->
  <ui-split-view column={open === null && agents.form === null ? 'sidebar' : 'detail'} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <ChatSidebar
        agents={agents.agents}
        chats={agents.chats}
        selected={open?.chat.id}
        onstart={(agent) => void agents.startChat(agent)}
        onedit={(agent) => agents.edit(agent)}
        onopen={(chat) => void agents.openChat(chat)}
      />
    </div>
    <div>
      {#if agents.form}
        <AgentForm
          bind:form={agents.form}
          failure={agents.failure}
          savable={agents.savable}
          onsave={() => void agents.save()}
          oncancel={() => (agents.form = null)}
          onremove={() => (agents.asking = 'agent')}
        />
      {:else if open}
        <ChatThread
          conversation={open}
          draft={agents.draft}
          failure={agents.failure}
          back={narrow}
          ondraft={(text) => (agents.draft = text)}
          onsend={() => void agents.send()}
          onstop={() => void agents.stop()}
          onanswer={(item, answer) => void agents.answer(item, answer)}
          onaction={(item, action) => void agents.act(item, action)}
          onremove={() => (agents.asking = 'chat')}
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
            message={agents.loaded && agents.agents.length === 0
              ? 'Add an agent to chat with: a name and its AG-UI endpoint.'
              : 'Pick an agent in the sidebar to start a chat.'}
            data-testid="agents-no-chat"
          >
            {#if agents.loaded && agents.agents.length === 0}
              <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <ui-button slot="action" variant="filled" hit onclick={() => agents.edit(null)} data-testid="agents-add-first">Add an agent</ui-button>
            {/if}
          </ui-empty-state>
        </div>
      {/if}
    </div>
  </ui-split-view>
  {#if agents.asking === 'chat' && open}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete this chat?"
      message="“{open.chat.title}” and everything in it is deleted."
      data-testid="agents-confirm"
      onconfirm={() => void agents.removeChat()}
      oncancel={() => (agents.asking = null)}
    ></ui-alert>
  {:else if agents.asking === 'agent' && agents.form?.agent}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {agents.form.agent.name}?"
      message="The agent and its token are removed from this machine. Its chats stay, and can be read."
      data-testid="agents-confirm"
      onconfirm={() => void agents.remove()}
      oncancel={() => (agents.asking = null)}
    ></ui-alert>
  {/if}
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
