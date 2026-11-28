<!-- @page AgentsPage — the whole app: the agents and chats beside the open chat, an agent's form or the type picker, or one of the two in a narrow window; what to do when Docker cannot be reached and a container agent needs it; driven by one window's Agents state -->
<script lang="ts">
  import { ChatThread } from '@vos/chat'
  import { EngineDown } from '@vos/docker'
  import Avatar from '../atoms/Avatar.svelte'
  import TypePicker from '../molecules/TypePicker.svelte'
  import AgentForm from '../organisms/AgentForm.svelte'
  import ChatSidebar from '../organisms/ChatSidebar.svelte'
  import type { Agents } from '../lib/agents.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    agents: Agents
    /** Open a web address the agent gave outside the app (the host's `open_link`). */
    onlink: (url: string) => void
  }

  let { agents, onlink }: Props = $props()

  // in a window too narrow for both, the split view shows one column: the kit says when, the same for every app
  let narrow = $state(false)
  const open = $derived(agents.open)
  const pane = $derived(agents.pane)
  // the agent of the open chat, while it is there
  const partner = $derived(open ? agents.agentOf(open.chat.agent_id) : null)

  $effect(() => {
    void agents.load()
    return agents.listen()
  })
</script>

<main class="agents" data-testid="agents-app">
  <!-- one column in a narrow window: the open chat, the type picker or the agent form is in front of the list -->
  <ui-split-view column={open === null && pane === null ? 'sidebar' : 'detail'} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <ChatSidebar
        agents={agents.agents}
        chats={agents.chats}
        selected={open?.chat.id}
        onstart={(agent) => void agents.startChat(agent)}
        onedit={(agent) => void agents.edit(agent)}
        onadd={() => agents.add()}
        onopen={(chat) => void agents.openChat(chat)}
      />
    </div>
    <div>
      {#if agents.dockerDown && agents.engine}
        <!-- a container agent's form and chats need Docker; the other agents never do -->
        <EngineDown
          engine={agents.engine}
          draft={agents.hostDraft}
          ondraft={(address) => (agents.hostDraft = address)}
          onconnect={() => void agents.connect()}
          testid="agents"
        />
      {:else if pane === 'types'}
        <TypePicker onpick={(kind) => void agents.edit(null, kind)} oncancel={() => agents.back()} />
      {:else if agents.form}
        <AgentForm
          bind:form={agents.form}
          failure={agents.failure}
          savable={agents.savable}
          secrets={agents.secrets}
          skills={agents.skills}
          images={agents.images}
          onsave={() => void agents.save()}
          oncancel={() => (agents.form = null)}
          onremove={() => (agents.asking = 'agent')}
        />
      {:else if open}
        <ChatThread
          conversation={open}
          partner={open.chat.agent}
          draft={agents.draft}
          failure={agents.failure}
          back={narrow}
          ondraft={(text) => (agents.draft = text)}
          onsend={() => void agents.send()}
          onstop={() => void agents.stop()}
          onanswer={(item, option) => void agents.answer(item, option.option_id)}
          onremove={() => (agents.asking = 'chat')}
          onback={() => agents.back()}
          {onlink}
          testid="agents"
        >
          {#snippet badge()}
            <span data-testid="agents-avatar-thread"
              ><Avatar id={open.chat.agent_id} name={open.chat.agent} picture={agents.avatarOf(open.chat)} size={24} /></span
            >
          {/snippet}
        </ChatThread>
      {:else}
        <div class="nothing">
          {#if agents.failure}
            <ui-banner tone="error" data-testid="agents-failure">{agents.failure}</ui-banner>
          {/if}
          <ui-empty-state
            icon="sparkles"
            heading="No chat open"
            message={agents.loaded && agents.agents.length === 0
              ? 'Add an agent to chat with: one at its ACP address, one in a container, or a program on this machine.'
              : 'Pick an agent in the sidebar to start a chat.'}
            data-testid="agents-no-chat"
          >
            {#if agents.loaded && agents.agents.length === 0}
              <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
              <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
              <ui-button slot="action" variant="filled" hit onclick={() => agents.add()} data-testid="agents-add-first">Add an agent</ui-button>
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
      message={partner?.kind === 'container'
        ? `“${open.chat.title}”, everything in it, and its container and data are deleted.`
        : `“${open.chat.title}” and everything in it is deleted.`}
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
      message={agents.form.kind === 'container'
        ? 'Its chats, with their containers and volumes, are removed with it. Its image stays, in Containers.'
        : agents.form.kind === 'program'
          ? 'The agent is removed from this machine. Its chats stay, and can be read.'
          : 'The agent and its token are removed from this machine. Its chats stay, and can be read.'}
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
