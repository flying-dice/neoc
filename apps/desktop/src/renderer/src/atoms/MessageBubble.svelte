<!-- @atom MessageBubble — iMessage-style chat bubble. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    from: 'user' | 'agent'
    children: Snippet
  }

  let { from, children }: Props = $props()
</script>

<div class="row {from}">
  <p class="bubble">{@render children()}</p>
</div>

<style>
  .row {
    display: flex;
    padding: 2px 16px;
  }
  .user {
    justify-content: flex-end;
  }
  .bubble {
    margin: 0;
    max-width: min(70%, 520px);
    padding: 7px 12px;
    border-radius: 18px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
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
  }
</style>
