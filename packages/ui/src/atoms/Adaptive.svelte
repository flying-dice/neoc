<!--
  @atom Adaptive — shows `compact` or `regular` by its own container width (so resizing a window never round-trips).
  Both branches are snippets; only the active one is mounted.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid?: string
    /** Container width in px at and above which `regular` is shown. */
    breakpoint?: number
    compact?: Snippet
    regular?: Snippet
    style?: string
  }

  let { testid, breakpoint = 560, compact, regular, style }: Props = $props()

  let width = $state(0)
  // Before the first measurement assume regular: the usual window is wide, and it avoids a compact flash.
  const isCompact = $derived(width > 0 && width < breakpoint)
</script>

<div class="ios-adaptive" data-testid={testid} bind:clientWidth={width} data-size={isCompact ? 'compact' : 'regular'} {style}>
  {#if isCompact}
    {@render compact?.()}
  {:else}
    {@render regular?.()}
  {/if}
</div>

<style>
  .ios-adaptive {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
</style>
