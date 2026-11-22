<!-- @molecule SessionBar — one line under the terminal that says where the session is, with the one thing to do about it -->
<script lang="ts">
  interface Props {
    /** What happened, as a sentence. */
    message: string
    /** The button's label; no button without one. */
    action?: string
    /** A spinner before the sentence: something is on its way. */
    busy?: boolean
    onaction?: () => void
  }

  let { message, action, busy = false, onaction }: Props = $props()
</script>

<div class="bar" role="status" data-testid="terminal-bar">
  {#if busy}<ui-spinner></ui-spinner>{/if}
  <p class="message" data-testid="terminal-bar-message">{message}</p>
  {#if action}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="tinted" size="small" onclick={() => onaction?.()} data-testid="terminal-bar-action">{action}</ui-button>
  {/if}
</div>

<style>
  .bar {
    box-sizing: border-box;
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 6px 12px;
    background: var(--bg-elevated, var(--bg));
    border-top: 1px solid var(--separator);
    color: var(--label);
  }
  .message {
    flex: 1 1 auto;
    min-width: 0;
    margin: 0;
    font: var(--type-footnote);
    overflow-wrap: anywhere;
  }
</style>
