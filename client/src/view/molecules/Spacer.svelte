<!-- @molecule Spacer — flexible empty space; `min` is its smallest size in pt. Atoms: Spacer. -->
<script lang="ts">
  import { Spacer } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const common = $derived(commonOf(ctx, node, path, scope))
  const min = $derived(Number(ctx.view.evaluate(node.props?.min, scope)))
</script>

<Spacer
  testid={common.testid}
  min={Number.isFinite(min) && min > 0 ? min : 0}
  style={common.style}
/>
