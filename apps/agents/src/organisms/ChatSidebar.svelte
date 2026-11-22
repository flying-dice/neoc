<!-- @organism ChatSidebar — who can be chatted with, and the chats so far; a tap on an agent starts a chat, on a chat opens it -->
<script lang="ts">
  import type { Agent, Chat } from 'app:rpc'

  interface Props {
    agents: readonly Agent[]
    /** The chats, the most recently used first. */
    chats: readonly Chat[]
    /** The id of the chat that is open. */
    selected?: string
    onstart: (agent: Agent) => void
    onopen: (chat: Chat) => void
  }

  let { agents, chats, selected, onstart, onopen }: Props = $props()
</script>

<nav class="sidebar" aria-label="Agents and chats">
  <ui-list variant="sidebar" label="Agents" data-testid="agents-roster">
    <ui-section header="Agents">
      {#each agents as agent (agent.id)}
        <ui-row
          heading={agent.name}
          subheading={agent.role}
          icon={agent.icon}
          icon-color="orange"
          tappable
          onpress={() => onstart(agent)}
          data-testid="agents-agent-{agent.id}"
        ></ui-row>
      {/each}
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
          ></ui-row>
        {/each}
      </ui-section>
    </ui-list>
  {:else}
    <ui-empty-state icon="sparkles" heading="No chats yet" message="Pick an agent above." data-testid="agents-no-chats"
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
</style>
