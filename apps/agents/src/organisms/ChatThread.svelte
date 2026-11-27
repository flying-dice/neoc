<!-- @organism ChatThread — one chat: its title, what was said and done in it (kept scrolled to the newest), and the composer under it -->
<script lang="ts">
  type Conversation = VosApp.Agents.Conversation
  type Item = VosApp.Agents.Item
  import type { PermissionOption } from '../lib/items'
  import ActivityCard from '../molecules/ActivityCard.svelte'
  import Composer from '../molecules/Composer.svelte'
  import MessageBubble from '../molecules/MessageBubble.svelte'
  import QuestionCard from '../molecules/QuestionCard.svelte'

  interface Props {
    conversation: Conversation
    /** What is typed so far. */
    draft: string
    /** Why the last call failed, shown over the composer. */
    failure?: string | null
    /** Show the way back to the list (a window too narrow for both). */
    back?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    /** The person answered a permission the agent asked for. */
    onanswer: (item: Item, option: PermissionOption) => void
    onremove: () => void
    onback?: () => void
  }

  let { conversation, draft, failure = null, back = false, ondraft, onsend, onstop, onanswer, onremove, onback }: Props = $props()

  let thread = $state<HTMLElement>()
  // everything the agent says or does moves the newest line: keep it in view
  const toNewest = (): void => thread?.scrollTo({ top: thread.scrollHeight })
  $effect(() => {
    void conversation.items
    toNewest()
  })
  // the lines change height when the fonts arrive, after the first scroll
  $effect(() => {
    void document.fonts.ready.then(toNewest)
  })
</script>

<section class="chat" aria-label={conversation.chat.title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the chats" onclick={onback} data-testid="agents-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Chats
      </ui-button>
    {/if}
    <h1 data-testid="agents-title">{conversation.chat.title}</h1>
    <p class="with">{conversation.chat.agent}</p>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" square destructive hit label="Delete this chat" onclick={onremove} data-testid="agents-remove-chat">
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
  </header>
  <!-- a keyboard reaches the log to scroll it -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="thread" bind:this={thread} role="log" tabindex="0" aria-label="Conversation" data-testid="agents-thread">
    {#each conversation.items as item (item.id)}
      {#if item.kind === 'user' || item.kind === 'assistant'}
        <MessageBubble {item} />
      {:else if item.kind === 'permission'}
        <QuestionCard {item} onanswer={(option) => onanswer(item, option)} />
      {:else if item.kind === 'error'}
        <ui-banner tone="error" data-testid="agents-item-error-{item.id}">{item.text}</ui-banner>
      {:else if item.kind === 'note'}
        <ui-banner tone="info" data-testid="agents-item-note-{item.id}">{item.text}</ui-banner>
      {:else}
        <ActivityCard {item} />
      {/if}
    {:else}
      <ui-empty-state icon="sparkles" heading="Chat with {conversation.chat.agent}" message="Write a message below."
      ></ui-empty-state>
    {/each}
  </div>
  {#if failure}
    <ui-banner tone="error" data-testid="agents-failure">{failure}</ui-banner>
  {/if}
  <Composer {draft} busy={conversation.busy} waiting={conversation.waiting} {ondraft} {onsend} {onstop} />
</section>

<style>
  .chat {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px 16px;
    border-bottom: 1px solid var(--separator);
  }
  h1 {
    margin: 0;
    min-width: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .with {
    margin: 0 0 0 auto;
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
  }
  .thread {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding: 16px;
  }
</style>
