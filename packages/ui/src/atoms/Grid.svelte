<!--
  @atom Grid — a grid of equally sized cells. `columns` / `rows` are CSS track lists (see `gridTemplate`);
  `cellAspect` (width / height) makes every cell a fixed ratio. Place children in `GridCell`s to span or pin them.
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
    /** Cell width / height. */
    cellAspect?: number
    style?: string
    children: Snippet
  }

  let { testid, columns, rows, rowGap = 0, colGap = 0, cellAspect, style, children }: Props = $props()
</script>

<div
  class="ios-grid"
  data-testid={testid}
  style:grid-template-columns={columns}
  style:grid-template-rows={rows}
  style:row-gap="{rowGap}px"
  style:column-gap="{colGap}px"
  style:--cell-aspect={cellAspect && cellAspect > 0 ? String(cellAspect) : undefined}
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
