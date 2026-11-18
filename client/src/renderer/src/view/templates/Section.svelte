<!--
  @template Section — a group of rows with a `header` and `footer`; `collapsible` turns the header into a disclosure
  button, and `expanded` is a Binding to client state (no round trip). Inside a `Form`, controls become rows.
  Organisms: ListSection. Molecules: FormCell. Atoms: Children.
-->
<script lang="ts">
  import { ListSection } from '@agentos/ui/organisms'
  import Children from '../atoms/Children.svelte'
  import FormCell from '../molecules/FormCell.svelte'
  import { useList, useView } from '../context'
  import { commonOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const list = useList()
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.expanded, scope))
  let local = $state(true)
  const collapsible = $derived(p('collapsible') === true)
  const expanded = $derived(!collapsible || (binding ? binding.value !== false : local))

  function change(next: boolean): void {
    if (binding) binding.set(next)
    else local = next
  }
</script>

<ListSection
  testid={common.testid}
  header={str(p('header')) || undefined}
  footer={str(p('footer')) || undefined}
  {collapsible}
  {expanded}
  onexpandedchange={change}
>
  {#if list?.cells()}
    {#each node.children ?? [] as child (child.key)}
      <FormCell node={child} {path} {scope} />
    {/each}
  {:else}
    <Children nodes={node.children} {path} {scope} />
  {/if}
</ListSection>
