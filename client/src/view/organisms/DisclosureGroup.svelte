<!--
  @organism DisclosureGroup — content that expands and collapses under a label (`label` prop, or the `label` slot).
  `expanded` is a Binding to client state (no round trip); without one the group keeps its own state. Molecules:
  DisclosureGroup. Atoms: Children.
-->
<script lang="ts">
  import { DisclosureGroup } from '@vos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.expanded, scope))
  let local = $state(false)
  const expanded = $derived(binding ? binding.value === true : local)
  const hasLabelSlot = $derived((node.slots?.label?.length ?? 0) > 0)
  const label = $derived(common.label ?? str(view.evaluate(node.props?.title, scope)))

  function change(next: boolean): void {
    if (binding) binding.set(next)
    else local = next
  }
</script>

{#snippet labelContent()}
  <Children nodes={node.slots?.label} {path} slotName="label" {scope} />
{/snippet}

<DisclosureGroup
  testid={common.testid}
  {label}
  {expanded}
  disabled={common.disabled}
  onchange={change}
  labelContent={hasLabelSlot ? labelContent : undefined}
>
  <Children nodes={node.children} {path} {scope} />
</DisclosureGroup>
