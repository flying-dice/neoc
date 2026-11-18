<!-- @atom MessageBubble — iMessage-style chat bubble. Test id: optional `testid` on the row. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    from: 'user' | 'agent'
    testid?: string
    children: Snippet
  }

  let { from, testid, children }: Props = $props()
</script>

<div class="row {from}" data-testid={testid}>
  <div class="bubble">{@render children()}</div>
</div>

<style>
  .row {
    display: flex;
    padding: 2px 16px;
    min-width: 0;
  }
  .user {
    justify-content: flex-end;
  }
  .bubble {
    margin: 0;
    max-width: min(78%, 560px);
    min-width: 0;
    padding: 7px 12px;
    border-radius: 18px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    overflow-wrap: anywhere;
    white-space: pre-wrap;
    -webkit-user-select: text;
    user-select: text;
  }
  .user .bubble {
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
    border-bottom-right-radius: 6px;
  }
  .agent .bubble {
    background: var(--fill-tertiary);
    color: var(--label);
    border-bottom-left-radius: 6px;
    white-space: normal;
  }
</style>
