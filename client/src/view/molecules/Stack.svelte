<!--
  @molecule Stack — VStack / HStack (`axis`). Spacing and padding are token units of 4 pt (0 to 8); apps never pass
  CSS. Atoms: Stack, Children.
-->
<script lang="ts">
  import { Stack } from '@agentos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf, unit } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const style = $derived(joinStyle(common.style, surfaceStyle(ctx, node, scope)))
</script>

<Stack
  testid={common.testid}
  axis={p('axis') === 'h' ? 'h' : 'v'}
  spacing={unit(p('spacing'))}
  align={oneOf(p('align'), ['start', 'center', 'end', 'stretch'] as const, 'stretch')}
  justify={oneOf(p('justify'), ['start', 'center', 'end', 'between', 'around'] as const, 'start')}
  wrap={p('wrap') === true}
  label={common.label}
  {style}
>
  <Children nodes={node.children} {path} {scope} />
</Stack>
