<!--
  @molecule Scope — re-points the colour scheme and tint for a subtree: the app's own canvas (a Calculator's black
  body). `scheme` is light or dark; `tint` (a palette name) and `background` come from the common surface props.
  Atoms: Scope, Children.
-->
<script lang="ts">
  import { Scope } from '@agentos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const common = $derived(commonOf(ctx, node, path, scope))
  const scheme = $derived(
    oneOf(ctx.view.evaluate(node.props?.scheme, scope), ['light', 'dark', 'none'] as const, 'none')
  )
</script>

<Scope
  testid={common.testid}
  scheme={scheme === 'none' ? undefined : scheme}
  style={joinStyle(common.style, surfaceStyle(ctx, node, scope))}
>
  <Children nodes={node.children} {path} {scope} />
</Scope>
