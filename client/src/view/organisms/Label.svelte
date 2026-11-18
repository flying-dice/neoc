<!--
  @organism Label — a title with an icon. `display` is `titleAndIcon`, `titleOnly` or `iconOnly` (the title stays
  the accessible name). Molecules: Label.
-->
<script lang="ts">
  import { Label } from '@agentos/ui/molecules'
  import { iconOf } from '../core/menuItems'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const COLORS = [
    'primary',
    'secondary',
    'tertiary',
    'tint',
    'destructive',
    'success',
    'warning'
  ] as const

  const common = $derived(commonOf(ctx, node, path, scope))
</script>

<span class="label" style={common.style} title={common.help}>
  <Label
    testid={common.testid}
    title={str(p('title'))}
    icon={iconOf(p('icon'))}
    display={oneOf(
      p('display'),
      ['titleAndIcon', 'titleOnly', 'iconOnly'] as const,
      'titleAndIcon'
    )}
    color={oneOf(p('foreground'), COLORS, 'primary')}
  />
</span>

<style>
  .label {
    display: inline-flex;
    min-width: 0;
  }
</style>
