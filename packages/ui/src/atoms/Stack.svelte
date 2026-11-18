<!--
  @atom Stack — a flex container along one axis (VStack / HStack). `spacing` and the alignment words are the only
  layout vocabulary; sizes are pixels the caller derived from the 4 pt grid. Optional test id: `testid`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    testid?: string
    axis?: 'v' | 'h'
    /** Gap in px (the caller maps token units to px). */
    spacing?: number
    align?: 'start' | 'center' | 'end' | 'stretch'
    justify?: 'start' | 'center' | 'end' | 'between' | 'around'
    /** Horizontal stacks only: flow onto new lines instead of overflowing. */
    wrap?: boolean
    label?: string
    /** Layout-only inline style from the caller (padding, frame, grow). */
    style?: string
    children: Snippet
  }

  let {
    testid,
    axis = 'v',
    spacing = 0,
    align = 'stretch',
    justify = 'start',
    wrap = false,
    label,
    style,
    children
  }: Props = $props()

  const ALIGN = { start: 'flex-start', center: 'center', end: 'flex-end', stretch: 'stretch' }
  const JUSTIFY = {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    between: 'space-between',
    around: 'space-around'
  }
</script>

<div
  class="ios-stack"
  class:row={axis === 'h'}
  class:wrap={wrap && axis === 'h'}
  data-testid={testid}
  role={label ? 'group' : undefined}
  aria-label={label}
  style:gap="{spacing}px"
  style:align-items={ALIGN[align]}
  style:justify-content={JUSTIFY[justify]}
  {style}
>
  {@render children()}
</div>

<style>
  .ios-stack {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .row {
    flex-direction: row;
  }
  .wrap {
    flex-wrap: wrap;
  }
</style>
