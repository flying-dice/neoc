<!-- @molecule MessageBubble — one thing said in a chat: the person's on the right, the agent's on the left (text, or an image, a link or a resource it gave), with a spinner while it is still coming -->
<script lang="ts">
  type Item = VosApp.Agents.Item
  import { dataOf, opens, pieceOf } from '../lib/items'

  interface Props {
    /** A `user` or an `assistant` item. */
    item: Item
  }

  let { item }: Props = $props()

  // the agent's text has no name; anything else it said is a piece in the item's data
  const piece = $derived(item.name === '' ? undefined : pieceOf(dataOf(item)))
</script>

<div class="message {item.kind}" data-testid="agents-message-{item.kind}-{item.id}">
  {#if piece?.type === 'image'}
    <img src="data:{piece.mime_type};base64,{piece.data}" alt="From the agent ({piece.mime_type})" />
  {:else if piece?.type === 'link'}
    {#if opens(piece.uri)}
      <a href={piece.uri} target="_blank" rel="noreferrer" data-testid="agents-message-{item.id}-link">{piece.name}</a>
    {:else}
      <span class="text">{piece.name}</span>
    {/if}
    {#if piece.name !== piece.uri}<span class="uri">{piece.uri}</span>{/if}
  {:else if piece?.type === 'resource'}
    <span class="uri">{piece.uri}</span>
    <pre>{piece.text}</pre>
  {:else}
    <span class="text">{item.text}</span>{#if !item.done}<ui-spinner label="The agent is answering"></ui-spinner>{/if}
  {/if}
</div>

<style>
  .message {
    margin: 0;
    max-width: min(85%, 60ch);
    padding: 8px 12px;
    border-radius: var(--radius-lg);
    overflow-wrap: anywhere;
  }
  .user {
    align-self: flex-end;
    /* the accent as a fill behind white text: darker than the accent itself in the dark appearance */
    background: var(--tint-fill);
    color: #fff;
  }
  .assistant {
    align-self: flex-start;
    background: var(--fill-tertiary);
  }
  /* the agent's line breaks are kept; the markup's own are not */
  .text {
    white-space: pre-wrap;
  }
  .uri {
    display: block;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  a {
    color: inherit;
    text-decoration: underline;
  }
  img {
    display: block;
    max-width: 100%;
    border-radius: var(--radius-sm, 4px);
  }
  pre {
    margin: 4px 0 0;
    max-height: 240px;
    overflow: auto;
    white-space: pre-wrap;
    font-family: ui-monospace, monospace;
    font-size: 12px;
  }
  ui-spinner {
    display: inline-block;
    vertical-align: middle;
  }
  .text:not(:empty) + ui-spinner {
    margin-inline-start: 6px;
  }
</style>
