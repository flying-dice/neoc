<!--
  @atom ZStack — overlays its children back to front in one grid cell, aligned to one of nine anchors.
  Optional test id: `testid`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  type Anchor =
    | 'topLeading'
    | 'top'
    | 'topTrailing'
    | 'leading'
    | 'center'
    | 'trailing'
    | 'bottomLeading'
    | 'bottom'
    | 'bottomTrailing'

  interface Props {
    testid?: string
    align?: Anchor
    style?: string
    children: Snippet
  }

  let { testid, align = 'center', style, children }: Props = $props()

  const HORIZONTAL = { Leading: 'start', Trailing: 'end' } as const
  const place = $derived({
    // `top`/`bottom` carry the row, a `Leading`/`Trailing` suffix the column.
    row: align.startsWith('top') ? 'start' : align.startsWith('bottom') ? 'end' : 'center',
    col: align.endsWith('Leading')
      ? HORIZONTAL.Leading
      : align.endsWith('Trailing') || align === 'trailing'
        ? HORIZONTAL.Trailing
        : align === 'leading'
          ? HORIZONTAL.Leading
          : 'center'
  })
</script>

<div
  class="ios-zstack"
  data-testid={testid}
  style:align-items={place.row}
  style:justify-items={place.col}
  {style}
>
  {@render children()}
</div>

<style>
  .ios-zstack {
    display: grid;
    min-width: 0;
  }
  /* Every child occupies the same cell: later children paint over earlier ones. */
  .ios-zstack > :global(*) {
    grid-area: 1 / 1;
    min-width: 0;
  }
</style>
