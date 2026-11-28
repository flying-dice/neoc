<!-- @organism ChatSidebar — who can be chatted with (each with its address, Container, or its command under its name), and the chats so far; a tap on an agent starts a chat, on a chat opens it -->
<script lang="ts">
  import Avatar from '../atoms/Avatar.svelte'
  import { subheadingOf } from '../lib/kinds'
  type Agent = VosApp.Agents.Agent
  type Chat = VosApp.Agents.Chat

  interface Props {
    agents: readonly Agent[]
    /** The chats, the most recently used first. */
    chats: readonly Chat[]
    /** The id of the chat that is open. */
    selected?: string
    onstart: (agent: Agent) => void
    /** Open the form to change the agent given. */
    onedit: (agent: Agent) => void
    /** Add an agent: its type is chosen first. */
    onadd: () => void
    onopen: (chat: Chat) => void
  }

  let { agents, chats, selected, onstart, onedit, onadd, onopen }: Props = $props()

  // each agent's picture, by its id: a chat shows its agent's
  const pictures = $derived(new Map(agents.map((a) => [a.id, a.avatar])))
</script>

<nav class="sidebar" aria-label="Agents and chats">
  <ui-list variant="sidebar" label="Agents" data-testid="agents-roster">
    <ui-section header="Agents">
      {#each agents as agent (agent.id)}
        <ui-row
          heading={agent.name}
          subheading={subheadingOf(agent)}
          tappable
          label="Start a chat with {agent.name}"
          onpress={() => onstart(agent)}
          data-testid="agents-agent-{agent.id}"
        >
          <!-- no space between what goes in the row's slots: a space would be its content, in place of its heading -->
          <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <ui-button
            slot="trailing"
            variant="plain"
            size="small"
            square
            hit
            label="Change {agent.name}"
            onclick={() => onedit(agent)}
            data-testid="agents-edit-{agent.id}"
          >
            <ui-icon slot="icon" name="ellipsis"></ui-icon>
          </ui-button><span slot="leading" class="avatar" data-testid="agents-avatar-agent-{agent.id}"
            ><Avatar id={agent.id} name={agent.name} picture={agent.avatar} size={32} /></span
          >
        </ui-row>
      {/each}
      <ui-row heading="Add an agent" action tappable onpress={onadd} data-testid="agents-add"></ui-row>
    </ui-section>
  </ui-list>
  <!-- an empty selectable list is not valid ARIA: the list is there once there is a chat -->
  {#if chats.length > 0}
    <ui-list variant="sidebar" selection="single" label="Chats" data-testid="agents-chats">
      <ui-section header="Chats">
        {#each chats as chat (chat.id)}
          <ui-row
            heading={chat.title}
            subheading={chat.agent}
            selected={selected === chat.id}
            onpress={() => onopen(chat)}
            data-testid="agents-chat-{chat.id}"
          >
            <span slot="leading" class="avatar" data-testid="agents-avatar-chat-{chat.id}">
              <Avatar id={chat.agent_id} name={chat.agent} picture={pictures.get(chat.agent_id) ?? ''} size={32} />
            </span>
          </ui-row>
        {/each}
      </ui-section>
    </ui-list>
  {:else}
    <ui-empty-state icon="sparkles" heading="No chats yet" message={agents.length === 0 ? 'Add an agent above to start one.' : 'Pick an agent above.'} data-testid="agents-no-chats"
    ></ui-empty-state>
  {/if}
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    overflow-y: auto;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 8px 12px;
    background: var(--bg-secondary);
  }
  .avatar {
    display: flex;
  }
</style>
