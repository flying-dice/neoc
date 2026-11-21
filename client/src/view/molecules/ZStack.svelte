<!--
  @molecule ZStack — overlays its children back to front, aligned to one of nine anchors. Atoms: ZStack, Children.
-->
<script lang="ts">
  import { ZStack } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()

  const common = $derived(commonOf(ctx, node, path, scope))
  const align = $derived(
    oneOf(
      ctx.view.evaluate(node.props?.align, scope),
      [
        'topLeading',
        'top',
        'topTrailing',
        'leading',
        'center',
        'trailing',
        'bottomLeading',
        'bottom',
        'bottomTrailing'
      ] as const,
      'center'
    )
  )
</script>

<ZStack
  testid={common.testid}
  {align}
  style={joinStyle(common.style, surfaceStyle(ctx, node, scope))}
>
  <Children nodes={node.children} {path} {scope} />
</ZStack>
