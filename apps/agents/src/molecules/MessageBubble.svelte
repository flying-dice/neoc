<!-- @molecule MessageBubble — one thing said in a chat: the person's on the right, the agent's on the left, with a spinner while it is still coming -->
<script lang="ts">
  import type { Item } from '@agents/rpc'

  interface Props {
    /** A `user` or an `assistant` item. */
    item: Item
  }

  let { item }: Props = $props()
</script>

<p class="message {item.kind}" data-testid="agents-message-{item.kind}-{item.id}">
  <span class="text">{item.text}</span>{#if !item.done}<ui-spinner label="The agent is answering"></ui-spinner>{/if}
</p>

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
  ui-spinner {
    display: inline-block;
    vertical-align: middle;
  }
  .text:not(:empty) + ui-spinner {
    margin-inline-start: 6px;
  }
</style>
