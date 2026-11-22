<!--
  @molecule Adaptive — chooses the `compact` or `regular` slot by its container width (`breakpoint`, default
  `COMPACT_BELOW`, 560 px), on the client, so resizing a window never round-trips. Atoms: Adaptive, Children.
-->
<script lang="ts">
  import { Adaptive } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf } from '../props'
  import type { AdapterProps } from '../registry'
  import { COMPACT_BELOW } from '../protocol'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const common = $derived(commonOf(ctx, node, path, scope))
  const breakpoint = $derived(Number(ctx.view.evaluate(node.props?.breakpoint, scope)))
</script>

{#snippet compact()}
  <Children nodes={node.slots?.compact} {path} slotName="compact" {scope} />
{/snippet}
{#snippet regular()}
  <Children nodes={node.slots?.regular} {path} slotName="regular" {scope} />
{/snippet}

<Adaptive
  testid={common.testid}
  breakpoint={Number.isFinite(breakpoint) && breakpoint > 0 ? breakpoint : COMPACT_BELOW}
  {compact}
  {regular}
  style={common.style}
/>
