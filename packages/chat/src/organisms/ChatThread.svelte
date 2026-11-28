<!-- @organism ChatThread — one chat: its title and who it is with, what was said and done in it (kept scrolled to the newest, each run of the agent's steps one row), what the agent is at while its turn runs, and the composer under it -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { Conversation, Item } from '../types'
  import { entriesOf, statusOf, type Entry, type PermissionOption } from '../lib/items'
  import StatusLine from '../atoms/StatusLine.svelte'
  import ToolStep from '../atoms/ToolStep.svelte'
  import Composer from '../molecules/Composer.svelte'
  import MessageBubble from '../molecules/MessageBubble.svelte'
  import PlanSteps from '../molecules/PlanSteps.svelte'
  import QuestionCard from '../molecules/QuestionCard.svelte'
  import StepGroup from '../molecules/StepGroup.svelte'

  interface Props {
    conversation: Conversation
    /** The name of who the chat is with: the agent, or the copilot. */
    partner: string
    /** What the bar shows before that name (the agent's picture); nothing without it. */
    badge?: Snippet
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
    /** Open a web address the agent gave outside the app. */
    onlink: (url: string) => void
    /** The app's word, which the test ids start with: `agents` gives `agents-thread`. */
    testid: string
  }

  let { conversation, partner, badge, draft, failure = null, back = false, ondraft, onsend, onstop, onanswer, onremove, onback, onlink, testid }: Props = $props()

  // a run of steps between the agent's words is one row
  const entries = $derived(entriesOf(conversation.items))
  const idOf = (entry: Entry): number => entry.item?.id ?? entry.steps![0]!.id
  const status = $derived(statusOf(conversation))

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
      <ui-button variant="plain" size="small" hit label="Back to the chats" onclick={onback} data-testid="{testid}-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Chats
      </ui-button>
    {/if}
    <h1 data-testid="{testid}-title">{conversation.chat.title}</h1>
    <p class="with">
      {@render badge?.()}
      {partner}
    </p>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" square destructive hit label="Delete this chat" onclick={onremove} data-testid="{testid}-remove-chat">
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
  </header>
  <!-- a keyboard reaches the log to scroll it -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="thread" bind:this={thread} role="log" tabindex="0" aria-label="Conversation" data-testid="{testid}-thread">
    {#each entries as entry (entry.steps ? `steps-${idOf(entry)}` : idOf(entry))}
      {#if entry.steps}
        <StepGroup steps={entry.steps} {onlink} {testid} />
      {:else}
        {@const item = entry.item}
        {#if item.kind === 'user' || item.kind === 'assistant'}
          <MessageBubble {item} {onlink} {testid} />
        {:else if item.kind === 'plan'}
          <PlanSteps {item} {testid} />
        {:else if item.kind === 'permission'}
          <QuestionCard {item} onanswer={(option) => onanswer(item, option)} {testid} />
        {:else if item.kind === 'error'}
          <ui-banner tone="error" data-testid="{testid}-item-error-{item.id}">{item.text}</ui-banner>
        {:else if item.kind === 'note'}
          <ui-banner tone="info" data-testid="{testid}-item-note-{item.id}">{item.text}</ui-banner>
        {:else}
          <ToolStep {item} {onlink} {testid} />
        {/if}
      {/if}
    {:else}
      <ui-empty-state icon="sparkles" heading="Chat with {partner}" message="Write a message below."
      ></ui-empty-state>
    {/each}
  </div>
  <StatusLine text={status} {testid} />
  {#if failure}
    <ui-banner tone="error" data-testid="{testid}-failure">{failure}</ui-banner>
  {/if}
  <Composer {draft} busy={conversation.busy} waiting={conversation.waiting} {ondraft} {onsend} {onstop} {testid} />
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
    display: flex;
    align-items: center;
    gap: 6px;
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
    gap: 12px;
    padding: 16px;
  }
  /* steps one after another (tools, thoughts, a plan) stack as one list */
  .thread > :global(.step + .step) {
    margin-top: -10px;
  }
</style>
