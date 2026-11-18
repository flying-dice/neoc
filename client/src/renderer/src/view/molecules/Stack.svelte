<!--
  @molecule Stack — VStack / HStack (`axis`). Spacing and padding are token units of 4 pt (0 to 8); apps never pass
  CSS. Atoms: Children.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf, unit } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const axis = $derived(p('axis') === 'h' ? 'row' : 'column')
  const spacing = $derived(unit(p('spacing')))
  const align = $derived(
    oneOf(p('align'), ['start', 'center', 'end', 'stretch'] as const, 'stretch')
  )
  const justify = $derived(
    oneOf(p('justify'), ['start', 'center', 'end', 'between', 'around'] as const, 'start')
  )
  const wrap = $derived(p('wrap') === true && axis === 'row')

  const ALIGN = {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    stretch: 'stretch'
  } as const
  const JUSTIFY = {
    start: 'flex-start',
    center: 'center',
    end: 'flex-end',
    between: 'space-between',
    around: 'space-around'
  } as const
</script>

<div
  class="stack"
  style:flex-direction={axis}
  style:gap="{spacing}px"
  style:align-items={ALIGN[align]}
  style:justify-content={JUSTIFY[justify]}
  style:flex-wrap={wrap ? 'wrap' : 'nowrap'}
  style={common.style}
>
  <Children nodes={node.children} {path} {scope} />
</div>

<style>
  .stack {
    display: flex;
    min-width: 0;
  }
</style>
