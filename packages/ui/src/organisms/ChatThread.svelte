<!--
  @organism ChatThread — the scrolling transcript of one chat: the person's bubbles, one activity line per turn
  (counts, never a list of tools), the agent's reply, permission requests and errors. It is a live log region, keeps the
  newest message in view while it streams (unless the reader scrolled up), and shows `empty` when there is nothing yet.
  Atoms: MessageBubble, RichText. Molecules: TurnActivity, PermissionCard.
  Test ids: `testid` on the log, `${testid}-user-<id>`, `${testid}-reply-<id>`, `${testid}-activity-<id>`,
  `${testid}-permission-<id>` (and its options), `${testid}-error-<id>`.
-->
<script lang="ts">
  import { tick, type Snippet } from 'svelte'
  import MessageBubble from '../atoms/MessageBubble.svelte'
  import RichText from '../atoms/RichText.svelte'
  import PermissionCard from '../molecules/PermissionCard.svelte'
  import TurnActivity from '../molecules/TurnActivity.svelte'
  import type { ChatBlock } from '../types'

  interface Props {
    testid: string
    blocks: ChatBlock[]
    /** The activity line that is live (the turn in progress). */
    liveActivityId?: string
    /** Overrides the live label (e.g. "Starting Claude…"). */
    status?: string
    /** A reply is streaming: the log is `aria-busy`, so a screen reader announces the finished message, not every token. */
    busy?: boolean
    /** Shown instead of the transcript when there are no blocks. */
    empty?: Snippet
    onchoose: (permissionId: string, optionId: string | null) => void
  }

  let { testid, blocks, liveActivityId, status, busy = false, empty, onchoose }: Props = $props()

  let scroller: HTMLElement | undefined = $state()
  let pinned = true

  function onscroll(): void {
    if (!scroller) return
    pinned = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 64
  }

  // Keep the newest message in view as the transcript streams, unless the reader scrolled up.
  $effect(() => {
    void blocks
    if (!pinned) return
    void tick().then(() => {
      if (scroller) scroller.scrollTop = scroller.scrollHeight
    })
  })
</script>

<!-- A scrolling region must be reachable by keyboard to be scrolled with it. -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<div
  class="thread"
  role="log"
  aria-live="polite"
  aria-relevant="additions text"
  aria-busy={busy}
  aria-label="Conversation"
  tabindex="0"
  data-testid={testid}
  bind:this={scroller}
  {onscroll}
>
  {#if blocks.length === 0 && empty}
    {@render empty()}
  {:else}
    <div class="blocks">
      {#each blocks as block (block.id)}
        {#if block.kind === 'user'}
          <MessageBubble from="user" testid="{testid}-user-{block.id}">{block.text}</MessageBubble>
        {:else if block.kind === 'activity'}
          <TurnActivity
            reasoning={block.reasoning}
            tools={block.tools}
            failed={block.failed}
            live={block.id === liveActivityId}
            status={block.id === liveActivityId ? status : undefined}
          />
        {:else if block.kind === 'reply'}
          <MessageBubble from="agent" testid="{testid}-reply-{block.id}"
            ><RichText text={block.text} /></MessageBubble
          >
        {:else if block.kind === 'permission'}
          <PermissionCard
            testid="{testid}-permission-{block.id}"
            title={block.title}
            options={block.options}
            answer={block.answer}
            onchoose={(optionId) => onchoose(block.id, optionId)}
          />
        {:else}
          <p class="notice" role="alert" data-testid="{testid}-error-{block.id}">{block.text}</p>
        {/if}
      {/each}
    </div>
  {/if}
</div>

<style>
  .thread {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .thread:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .blocks {
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 12px 0 8px;
  }
  .notice {
    margin: 4px 16px;
    font: var(--type-footnote);
    color: var(--ios-red);
    overflow-wrap: anywhere;
  }
</style>
