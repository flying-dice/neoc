<!-- @molecule Divider — a hairline separator (role separator); vertical inside a horizontal stack. Atoms: Divider. -->
<script lang="ts">
  import { Divider } from '@agentos/ui/atoms'
  import { useView } from '../context'
  import { commonOf } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const common = $derived(commonOf(ctx, node, path, scope))
</script>

<Divider testid={common.testid} style={common.style} />
