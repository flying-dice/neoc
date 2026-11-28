<!-- @organism CopilotSidebar — the copilots, each with a new chat, its settings and its chats so far; and under them the way to add a copilot and the skill library -->
<script lang="ts">
  type Chat = VosApp.Copilot.Chat
  type Copilot = VosApp.Copilot.Copilot

  interface Props {
    copilots: readonly Copilot[]
    /** The chats, the most recently used first. */
    chats: readonly Chat[]
    /**
     * What the detail column shows, to be marked: `chat:<id>`, `new:<copilot>` (a new chat nothing was said in yet),
     * `settings:<copilot>`, `add` or `skills`; nothing is marked when left out.
     */
    selected?: string
    onstart: (copilot: Copilot) => void
    /** Open the form: to change the copilot given, or to add one (null). */
    onedit: (copilot: Copilot | null) => void
    onopen: (chat: Chat) => void
    onlibrary: () => void
  }

  let { copilots, chats, selected, onstart, onedit, onopen, onlibrary }: Props = $props()

  // each copilot's chats, in the order they come
  const chatsOf = $derived.by(() => {
    const by = new Map<string, Chat[]>()
    for (const chat of chats) by.set(chat.copilot_id, [...(by.get(chat.copilot_id) ?? []), chat])
    return (copilot: string): Chat[] => by.get(copilot) ?? []
  })
</script>

<nav class="sidebar" aria-label="Copilots and chats">
  {#each copilots as copilot (copilot.id)}
    <!-- each row shows something in the detail column: a selectable list -->
    <ui-list variant="sidebar" selection="single" label={copilot.name} data-testid="copilot-copilot-{copilot.id}">
      <ui-section header={copilot.name}>
        <ui-row
          heading="New chat"
          icon="plus"
          icon-color="blue"
          label="New chat with {copilot.name}"
          selected={selected === `new:${copilot.id}`}
          onpress={() => onstart(copilot)}
          data-testid="copilot-new-chat-{copilot.id}"
        ></ui-row>
        <ui-row
          heading="Settings"
          icon="gear"
          label="Settings of {copilot.name}"
          selected={selected === `settings:${copilot.id}`}
          onpress={() => onedit(copilot)}
          data-testid="copilot-edit-{copilot.id}"
        ></ui-row>
        {#each chatsOf(copilot.id) as chat (chat.id)}
          <ui-row heading={chat.title} selected={selected === `chat:${chat.id}`} onpress={() => onopen(chat)} data-testid="copilot-chat-{chat.id}"></ui-row>
        {/each}
      </ui-section>
    </ui-list>
  {/each}
  <ui-list variant="sidebar" selection="single" label="Library" data-testid="copilot-library">
    <ui-section header={copilots.length === 0 ? 'Copilots' : undefined}>
      <ui-row heading="Add a copilot" icon="plus" icon-color="green" selected={selected === 'add'} onpress={() => onedit(null)} data-testid="copilot-add"></ui-row>
      <ui-row heading="Skills" icon="doc" icon-color="orange" selected={selected === 'skills'} onpress={onlibrary} data-testid="copilot-open-skills"></ui-row>
    </ui-section>
  </ui-list>
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
