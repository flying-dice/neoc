<!--
  @atom Grid — a grid of equally sized cells. `columns` / `rows` are CSS track lists (see `gridTemplate`);
  Give each `GridCell` the grid's `cellAspect` (width / height of one cell) and its `span` to make cells a fixed ratio
  (a spanning cell keeps the row's height), and place children in `GridCell`s to span or pin them.
  Optional test id: `testid`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid?: string
    columns?: string
    rows?: string
    /** Gaps in px. */
    rowGap?: number
    colGap?: number
    style?: string
    children: Snippet
  }

  let { testid, columns, rows, rowGap = 0, colGap = 0, style, children }: Props = $props()
</script>

<div
  class="ios-grid"
  data-testid={testid}
  style:grid-template-columns={columns}
  style:grid-template-rows={rows}
  style:row-gap="{rowGap}px"
  style:column-gap="{colGap}px"
  style:--grid-col-gap="{colGap}px"
  {style}
>
  {@render children()}
</div>

<style>
  .ios-grid {
    display: grid;
    min-width: 0;
  }
</style>
