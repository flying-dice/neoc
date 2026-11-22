<!--
  @template ScreenTemplate — a single scrolling screen: navigation bar on top, scrollable content.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    background?: 'grouped' | 'plain'
    navigationBar?: Snippet
    content: Snippet
  }

  let { background = 'grouped', navigationBar, content }: Props = $props()
</script>

<div class="screen {background}">
  <div class="scroll" class:has-bar={!!navigationBar}>
    {#if navigationBar}{@render navigationBar()}{/if}
    <div class="content">{@render content()}</div>
  </div>
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
    overscroll-behavior-y: contain;
  }
  /* The navigation bar sticks to the top (--nav-bar-height): a row scrolled to the top stops below it. */
  .has-bar {
    scroll-padding-top: var(--nav-bar-height);
  }
  .content {
    padding-bottom: 24px;
  }
</style>
