<!--
  @molecule Node — one wire node: looks its `type` up in the window's component registry and renders the adapter.
  A type this client does not know renders the node's own `fallback` slot if it has one (spec 2.7), else
  `Unsupported`. `hidden` removes the node from layout and the accessibility tree. A node with a `contextMenu` slot or `drag` / `drop`
  props is wrapped by the registry's decorator. Atoms: Children, Unsupported.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import Unsupported from '../atoms/Unsupported.svelte'
  import { testid } from '../core/testid'
  import { needsDecorator } from '../registry'
  import type { Scope } from '../core/view'
  import { useView } from '../context'
  import type { Node } from '../protocol'

  interface Props {
    node: Node
    path: string
    scope: Scope
  }

  let { node, path, scope }: Props = $props()
  const ctx = useView()

  const spec = $derived(ctx.registry.get(node.type))
  const hidden = $derived(ctx.view.evaluate(node.props?.hidden, scope) === true)
  const fallback = $derived(node.slots?.fallback ?? [])
  const Decorator = $derived(needsDecorator(node) ? ctx.registry.decorator : null)
</script>

{#if !hidden}
  {#if spec}
    {@const Adapter = spec.component}
    {#if Decorator}
      <Decorator {node} {path} {scope}>
        {#snippet children()}<Adapter {node} {path} {scope} />{/snippet}
      </Decorator>
    {:else}
      <Adapter {node} {path} {scope} />
    {/if}
  {:else if fallback.length}
    <Children nodes={fallback} {path} slotName="fallback" {scope} />
  {:else}
    <Unsupported
      type={node.type}
      testid={testid(ctx.app, path, scope.rowKey, node.props?.testid)}
    />
  {/if}
{/if}
