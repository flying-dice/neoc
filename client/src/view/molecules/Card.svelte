<!--
  @molecule Card — a surface that groups content: `size` (the widget proportions or `auto`) and `surface` (solid or
  material). Atoms: Card, Children.
-->
<script lang="ts">
  import { Card } from '@agentos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)
  const common = $derived(commonOf(ctx, node, path, scope))
</script>

<Card
  testid={common.testid}
  size={oneOf(p('size'), ['small', 'medium', 'large', 'auto'] as const, 'auto')}
  surface={oneOf(p('surface'), ['solid', 'material'] as const, 'solid')}
>
  <Children nodes={node.children} {path} {scope} />
</Card>
