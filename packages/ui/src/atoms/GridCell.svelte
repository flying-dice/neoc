<!--
  @atom GridCell — one cell of a `Grid`: pins or spans its content (`column`, `row` are CSS grid-line shorthands)
  and applies the grid's `cellAspect`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    column?: string
    row?: string
    children: Snippet
  }

  let { column, row, children }: Props = $props()
</script>

<div class="ios-grid-cell" style:grid-column={column} style:grid-row={row}>
  {@render children()}
</div>

<style>
  .ios-grid-cell {
    min-width: 0;
    min-height: 0;
    aspect-ratio: var(--cell-aspect, auto);
    display: flex;
    flex-direction: column;
  }
  .ios-grid-cell > :global(*) {
    flex: 1;
  }
</style>
