<!--
  @organism Sheet — a modal sheet presented through `presented` (a bound bool) or `item` (presented while it holds a
  value). A portal node: it renders nothing in flow and mounts in the window's overlay layer, with the window
  content inert behind it. `style` is `form` (centred), `page` (bottom; `detents` small/medium/large make it
  resizable) or `fullScreen`. Focus moves in and stays in, Escape and the scrim dismiss unless `dismissible` is
  false, and focus returns to the opener. Dismissing sets the binding false and fires `dismiss`. Molecules: Sheet.
  Atoms: Children.
-->
<script lang="ts">
  import { Sheet } from '@agentos/ui/molecules'
  import { normaliseDetents } from '@agentos/ui'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { presented } from '../kit/presented.svelte'
  import { modalWhile, portalToWindow } from '../overlay'

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
  const style = $derived(oneOf(p('style'), ['form', 'page', 'fullScreen'] as const, 'form'))
  const detents = $derived(Array.isArray(p('detents')) ? normaliseDetents(p('detents')) : undefined)
</script>

<div class="portal" {@attach portalToWindow(ctx.app, (o) => (origin = o))}>
  {#if present.open}<span hidden {@attach modalWhile(ctx.app)}></span>{/if}
  <Sheet
    testid={common.testid}
    open={present.open}
    presentation={style}
    {detents}
    dismissible={p('dismissible') !== false}
    label={common.label ?? 'Sheet'}
    onclose={() => present.dismiss()}
  >
    <Children nodes={node.children} {path} {scope} />
  </Sheet>
</div>

<style>
  .portal {
    display: contents;
  }
</style>
