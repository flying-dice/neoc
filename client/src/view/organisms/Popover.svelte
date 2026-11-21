<!--
  @organism Popover — a surface anchored to a node (`anchor` is its key; default the node it was written inside),
  with an arrow unless `arrow` is false, on the requested `edge` (flipping when there is no room). A portal node
  mounted in the window's overlay layer. On a compact window it becomes a bottom sheet. Escape or a press outside
  dismisses (sets `presented` false, fires `dismiss`), and focus returns to the opener. Molecules: Popover.
  Atoms: Children.
-->
<script lang="ts">
  import { Popover } from '@vos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { optStr } from '../kit/support'
  import { presented } from '../kit/presented.svelte'
  import { anchorElement, portalToWindow } from '../overlay'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const p = (name: string): unknown => ctx.view.evaluate(node.props?.[name], scope)

  let origin: HTMLElement | null = $state(null)
  const present = presented({
    ctx,
    node: () => node,
    path: () => path,
    scope: () => scope,
    host: () => origin
  })
  const common = $derived(commonOf(ctx, node, path, scope))
  const anchor = $derived(present.open ? anchorElement(ctx, optStr(p('anchor')), origin) : null)
  const compact = $derived(ctx.view.store.env.sizeClass === 'compact')
</script>

<div class="portal" {@attach portalToWindow(ctx.app, (o) => (origin = o))}>
  <Popover
    testid={common.testid}
    open={present.open}
    {anchor}
    edge={oneOf(p('edge'), ['top', 'bottom', 'leading', 'trailing'] as const, 'bottom')}
    arrow={p('arrow') !== false}
    {compact}
    label={common.label ?? 'Popover'}
    onclose={() => present.dismiss()}
  >
    <Children nodes={node.children} {path} {scope} />
  </Popover>
</div>

<style>
  .portal {
    display: contents;
  }
</style>
