<!--
  @molecule Text — wire props to the kit's Text atom. Content is always a text node, never HTML (agent and tool
  text is untrusted). `style` is a closed text style (including the `display` readout, with `fit`), `foreground` a
  label token, `heading` the semantic level (h1..h6), `live` a live region, `lines`/`truncation` the ellipsis,
  `selectable` allows copying, `digits = tabular` keeps numbers from jittering. Atoms: Text.
-->
<script lang="ts">
  import { Text } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle } from '../kit/support'

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
    'caption2',
    'display'
  ] as const
  const COLORS = [
    'primary',
    'secondary',
    'tertiary',
    'tint',
    'destructive',
    'success',
    'warning'
  ] as const
  const WEIGHTS = ['regular', 'medium', 'semibold', 'bold'] as const

  const common = $derived(commonOf(ctx, node, path, scope))
  const text = $derived(str(p('text')))
  const style = $derived(oneOf(p('style'), STYLES, 'body'))
  const color = $derived(
    p('foreground') === undefined ? 'inherit' : oneOf(p('foreground'), COLORS, 'primary')
  )
  const weight = $derived(
    p('weight') === undefined ? undefined : oneOf(p('weight'), WEIGHTS, 'regular')
  )
  const align = $derived(oneOf(p('align'), ['leading', 'center', 'trailing'] as const, 'leading'))
  const lines = $derived(Math.max(0, Math.round(Number(p('lines')) || 0)))
  const heading = $derived(Math.round(Number(p('heading'))))
  const live = $derived(oneOf(p('live'), ['off', 'polite', 'assertive'] as const, 'off'))
  const mono = $derived(p('design') === 'mono')
  const rounded = $derived(p('design') === 'rounded')
  const fit = $derived(p('fit') === true && style === 'display')
  const isHeading = $derived(heading >= 1 && heading <= 6)
  /** `tint`, `background` and `corner`; the text colour is the `foreground` token handled above. */
  const appearance = $derived(appearanceStyle(ctx, node, scope))
</script>

<span
  class="text"
  style:text-align={align === 'leading' ? undefined : align === 'trailing' ? 'end' : 'center'}
  style="{common.style};{appearance}"
  title={common.help}
  role={isHeading ? 'heading' : undefined}
  aria-level={isHeading ? heading : undefined}
  aria-live={live === 'off' ? undefined : live}
  data-testid={common.testid}
>
  <Text
    {style}
    {color}
    {weight}
    {mono}
    {rounded}
    {fit}
    lines={lines > 0 ? lines : undefined}
    selectable={p('selectable') === true}
    tabular={p('digits') === 'tabular'}>{text}</Text
  >
</span>

<style>
  .text {
    display: block;
    min-width: 0;
  }
</style>
