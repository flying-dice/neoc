<!--
  @molecule Icon — a glyph from the kit's icon set. An unknown name renders nothing visible but keeps its box, so a
  newer app degrades without shifting the layout. With a `label` it is an image with that name; without one it is
  decoration and hidden from assistive technology. Atoms: Icon.
-->
<script lang="ts">
  import { Icon } from '@agentos/ui/atoms'
  import { iconOf } from '../core/menuItems'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { appearanceStyle, num } from '../kit/support'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  const WEIGHT = { regular: 2, medium: 2.3, semibold: 2.6, bold: 3 } as const

  const common = $derived(commonOf(ctx, node, path, scope))
  const name = $derived(iconOf(p('name')))
  const size = $derived(Math.min(96, Math.max(8, num(p('size'), 20))))
  const weight = $derived(WEIGHT[oneOf(p('weight'), ['regular', 'medium', 'semibold', 'bold'] as const, 'regular')])
  const appearance = $derived(appearanceStyle(ctx, node, scope))
</script>

<span
  class="icon"
  style="{common.style};{appearance};width:{size}px;height:{size}px"
  title={common.help}
  data-testid={common.testid}
  role={common.label ? 'img' : undefined}
  aria-label={common.label}
  aria-hidden={common.label ? undefined : 'true'}
>
  {#if name}<Icon {name} {size} {weight} />{/if}
</span>

<style>
  .icon {
    display: inline-grid;
    place-items: center;
    flex: none;
    color: var(--tint);
  }
</style>
