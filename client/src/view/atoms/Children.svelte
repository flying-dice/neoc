<!--
  @atom Children — renders a list of child nodes (the default slot, or a named one) through the view's `render`
  snippet. Atoms import no components, so the recursion goes through the context, not through `Node`.
-->
<script lang="ts">
  import type { Scope } from '../core/view'
  import { childPath } from '../core/tree'
  import { useView } from '../context'
  import type { Node } from '../protocol'

  interface Props {
    nodes: Node[] | undefined
    /** Path of the node that owns these children. */
    path: string
    /** A named slot (`toolbar.trailing`, `row`); omit for the default slot. */
    slotName?: string
    scope: Scope
  }

  let { nodes, path, slotName, scope }: Props = $props()
  const ctx = useView()
</script>

{#each nodes ?? [] as child (child.key)}
  {@render ctx.render(child, childPath(path, child.key, slotName), scope)}
{/each}
