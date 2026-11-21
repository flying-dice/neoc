<!--
  @atom GridCell — one cell of a `Grid`: pins or spans its content (`column`, `row` are CSS grid-line shorthands)
  and gives the row its height from the grid's `cellAspect` (width / height of ONE cell). A cell that spans several
  columns is wider than one cell plus the gaps between them, so its aspect cannot be a plain multiple: it keeps the
  row's height through a hidden strut one cell wide (the span's width less its gaps, over the span) with that aspect.
  Needs the grid's column gap as `--grid-col-gap` (the Grid atom sets it).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    column?: string
    row?: string
    /** Columns spanned (default 1). */
    span?: number
    /** Width / height of one cell of the grid (its `cellAspect`). */
    aspect?: number
    children: Snippet
  }

  let { column, row, span = 1, aspect, children }: Props = $props()

  const ratio = $derived(aspect && aspect > 0 ? aspect : undefined)
  const spans = $derived(span > 1)
</script>

<div
  class="ios-grid-cell"
  class:spans
  style:grid-column={column}
  style:grid-row={row}
  style:aspect-ratio={ratio && !spans ? String(ratio) : undefined}
  style:--span={spans ? span : undefined}
  style:--aspect={ratio}
>
  {#if spans && ratio}
    <div class="strut" aria-hidden="true"></div>
  {/if}
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
  /* One grid area holds the strut and the content, so the content fills what the strut makes tall. */
  .spans {
    display: grid;
  }
  .spans > :global(*) {
    grid-area: 1 / 1;
    min-width: 0;
    min-height: 0;
  }
  .strut {
    visibility: hidden;
    pointer-events: none;
    width: calc((100% - (var(--span) - 1) * var(--grid-col-gap, 0px)) / var(--span));
    aspect-ratio: var(--aspect);
  }
</style>
