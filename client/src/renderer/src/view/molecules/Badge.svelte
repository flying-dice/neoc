<!--
  @molecule Badge — a count or short label; a dot when `value` is omitted. `max` caps a number ("99+"), `tint`
  re-points the colour (default red). The text colour is chosen by the kit for 4.5:1. A dot has no text, so it
  needs a `label` for assistive technology. Atoms: Badge.
-->
<script lang="ts">
  import { Badge } from '@agentos/ui/atoms'
  import { isPaletteToken } from '@agentos/ui'
  import { useView } from '../context'
  import { commonOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { num } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const raw = $derived(p('value'))
  const value = $derived(
    typeof raw === 'number' || (typeof raw === 'string' && raw !== '') ? raw : undefined
  )
  const tint = $derived(isPaletteToken(p('tint')) ? `var(--ios-${p('tint') as string})` : undefined)
</script>

<span
  class="badge"
  style={common.style}
  title={common.help}
  data-testid={common.testid}
  role={common.label ? 'img' : undefined}
  aria-label={common.label}
>
  <Badge {value} color={tint} max={Math.max(1, Math.round(num(p('max'), 99)))} />
</span>

<style>
  .badge {
    display: inline-flex;
    flex: none;
  }
</style>
