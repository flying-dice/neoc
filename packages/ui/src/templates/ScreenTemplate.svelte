<!--
  @template ScreenTemplate — a single scrolling screen: navigation bar on top,
  scrollable content, optional bottom bar (tab bar / toolbar / composer).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    background?: 'grouped' | 'plain'
    navigationBar?: Snippet
    content: Snippet
    bottomBar?: Snippet
  }

  let { background = 'grouped', navigationBar, content, bottomBar }: Props = $props()
</script>

<div class="screen {background}">
  <div class="scroll">
    {#if navigationBar}{@render navigationBar()}{/if}
    <div class="content">{@render content()}</div>
  </div>
  {#if bottomBar}<div class="bottom">{@render bottomBar()}</div>{/if}
</div>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .grouped {
    background: var(--bg-grouped-elevated);
  }
  .plain {
    background: var(--bg-elevated);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
  .content {
    padding-bottom: 24px;
  }
  .bottom {
    flex: none;
  }
</style>
