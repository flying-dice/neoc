<!-- @molecule MessageBubble — one thing said in a chat: the person's in a bubble on the right, the agent's across the column with no bubble (its text drawn as Markdown, or an image, a link or a resource it gave) -->
<script lang="ts">
  import type { Item } from '../types'
  import Markdown from '../atoms/Markdown.svelte'
  import { dataOf, opens, pieceOf } from '../lib/items'

  interface Props {
    /** A `user` or an `assistant` item. */
    item: Item
    /** Open a web address the agent gave outside the app. */
    onlink: (url: string) => void
    /** The app's word, which the test ids start with: `agents` gives `agents-message-user-1`. */
    testid: string
  }

  let { item, onlink, testid }: Props = $props()

  // the agent's text has no name; anything else it said is a piece in the item's data
  const piece = $derived(item.name === '' ? undefined : pieceOf(dataOf(item)))
</script>

<div class="message {item.kind}" data-testid="{testid}-message-{item.kind}-{item.id}">
  {#if item.kind === 'user'}
    <span class="text">{item.text}</span>
  {:else if piece?.type === 'image'}
    <img src="data:{piece.mime_type};base64,{piece.data}" alt="From the agent ({piece.mime_type})" />
  {:else if piece?.type === 'link'}
    {#if opens(piece.uri)}
      <a
        href={piece.uri}
        target="_blank"
        rel="noopener noreferrer"
        onclick={(e) => {
          // the frame cannot open a tab of its own: the host opens it
          e.preventDefault()
          onlink(piece.uri)
        }}
        data-testid="{testid}-message-{item.id}-link">{piece.name}</a
      >
    {:else}
      <span class="text">{piece.name}</span>
    {/if}
    {#if piece.name !== piece.uri}<span class="uri">{piece.uri}</span>{/if}
  {:else if piece?.type === 'resource'}
    <span class="uri">{piece.uri}</span>
    <pre>{piece.text}</pre>
  {:else}
    <Markdown text={item.text} {onlink} testid="{testid}-message-{item.id}-text" />
  {/if}
</div>

<style>
  .message {
    margin: 0;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  /* the person's: a bubble on the right, a neutral fill that reads in both appearances */
  .user {
    align-self: flex-end;
    max-width: min(85%, 60ch);
    padding: 8px 16px;
    border-radius: var(--radius-xl);
    background: var(--fill-tertiary);
    color: var(--label);
  }
  /* the agent's: the column's whole width, as a page reads */
  .assistant {
    align-self: stretch;
  }
  /* the person's line breaks are kept; the markup's own are not */
  .text {
    white-space: pre-wrap;
  }
  .uri {
    display: block;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  a {
    color: var(--link);
    text-decoration: underline;
    text-underline-offset: 2px;
  }
  img {
    display: block;
    max-width: 100%;
    border-radius: var(--radius-sm);
  }
  pre {
    margin: 4px 0 0;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    font: var(--type-footnote);
    font-family: var(--font-mono);
  }
</style>
