<!--
  @molecule LabeledContent — a label with a value (`value`) or content (children) beside it. Atoms:
  LabeledContent, Children.
-->
<script lang="ts">
  import { LabeledContent } from '@vos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const common = $derived(commonOf(ctx, node, path, scope))
  const value = $derived(ctx.view.evaluate(node.props?.value, scope))
  const hasContent = $derived((node.children?.length ?? 0) > 0)
</script>

{#snippet content()}
  <Children nodes={node.children} {path} {scope} />
{/snippet}

<LabeledContent
  testid={common.testid}
  label={common.label ?? str(ctx.view.evaluate(node.props?.title, scope))}
  value={value === undefined || value === null ? undefined : str(value)}
  style={common.style}
  children={hasContent ? content : undefined}
/>
