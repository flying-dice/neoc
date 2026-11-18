<!--
  @atom GridCell — one cell of a `Grid`: pins or spans its content (`column`, `row` are CSS grid-line shorthands)
  and applies its aspect ratio (the grid's `cellAspect`, scaled by the span).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    column?: string
    row?: string
    /** Width / height of this cell; a spanning cell scales the grid's `cellAspect` by its span (`spannedAspect`). */
    aspect?: number
    children: Snippet
  }

  let { column, row, aspect, children }: Props = $props()
</script>

<div
  class="ios-grid-cell"
  style:grid-column={column}
  style:grid-row={row}
  style:aspect-ratio={aspect && aspect > 0 ? String(aspect) : undefined}
>
  {@render children()}
</div>

<style>
  .ios-grid-cell {
    min-width: 0;
    min-height: 0;
    display: flex;
    flex-direction: column;
  }
  .ios-grid-cell > :global(*) {
    flex: 1;
  }
</style>
