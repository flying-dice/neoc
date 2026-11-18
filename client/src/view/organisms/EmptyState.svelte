<!--
  @organism EmptyState — what a screen shows when there is nothing to show: `icon`, `title`, `message`, and an
  optional call to action in the `action` slot (its tap also reports `onAction`). Molecules: EmptyState. Atoms:
  Children.
-->
<script lang="ts">
  import { icons, type IconName } from '@agentos/ui'
  import { EmptyState } from '@agentos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx

  const common = $derived(commonOf(ctx, node, path, scope))
  const icon = $derived(view.evaluate(node.props?.icon, scope))
  const message = $derived(str(view.evaluate(node.props?.message, scope)) || undefined)
  const hasAction = $derived((node.slots?.action?.length ?? 0) > 0)
</script>

{#snippet action()}
  <Children nodes={node.slots?.action} {path} slotName="action" {scope} />
{/snippet}

<EmptyState
  testid={common.testid}
  title={str(view.evaluate(node.props?.title, scope))}
  {message}
  icon={typeof icon === 'string' && icon in icons ? (icon as IconName) : undefined}
  style={common.style}
  action={hasAction ? action : undefined}
/>
