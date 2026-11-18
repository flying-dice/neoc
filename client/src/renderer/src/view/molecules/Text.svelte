<!--
  @molecule Text — wire props to the kit's Text atom. Content is always a text node, never HTML (agent and tool
  text is untrusted). `heading` sets the semantic level, `live` a live region. Atoms: Text.
-->
<script lang="ts">
  import { Text } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const STYLES = [
    'largeTitle',
    'title1',
    'title2',
    'title3',
    'headline',
    'body',
    'callout',
    'subheadline',
    'footnote',
    'caption1',
    'caption2'
  ] as const
  const COLORS = ['primary', 'secondary', 'tertiary', 'tint', 'destructive'] as const
  const WEIGHTS = ['regular', 'medium', 'semibold', 'bold'] as const

  const common = $derived(commonOf(ctx, node, path, scope))
  const text = $derived(str(p('text')))
  const style = $derived(oneOf(p('style'), STYLES, 'body'))
  const color = $derived(oneOf(p('foreground'), COLORS, 'primary'))
  const weight = $derived(
    p('weight') === undefined ? undefined : oneOf(p('weight'), WEIGHTS, 'regular')
  )
  const align = $derived(oneOf(p('align'), ['leading', 'center', 'trailing'] as const, 'leading'))
  const lines = $derived(Number(p('lines')))
  const heading = $derived(Math.round(Number(p('heading'))))
  const live = $derived(oneOf(p('live'), ['off', 'polite', 'assertive'] as const, 'off'))
  const mono = $derived(p('design') === 'mono')
  const rounded = $derived(p('design') === 'rounded')
</script>

<span
  class="text"
  style:text-align={align === 'leading' ? undefined : align === 'trailing' ? 'end' : 'center'}
  style={common.style}
  title={common.help}
  role={heading >= 1 && heading <= 6 ? 'heading' : undefined}
  aria-level={heading >= 1 && heading <= 6 ? heading : undefined}
  aria-live={live === 'off' ? undefined : live}
  data-testid={common.testid}
>
  <Text {style} {color} {weight} {mono} {rounded} truncate={lines === 1}>{text}</Text>
</span>

<style>
  .text {
    display: block;
    min-width: 0;
  }
</style>
