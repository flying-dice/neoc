<!--
  @molecule Show — conditional content, decided on the client: `when` is an expression over refs, so toggling it
  never round-trips. Children are the "then" branch; the `otherwise` slot is the other. Atoms: Children.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()

  const when = $derived(Boolean(ctx.view.evaluate(node.props?.when, scope)))
</script>

<div class="show">
  {#if when}
    <Children nodes={node.children} {path} {scope} />
  {:else}
    <Children nodes={node.slots?.otherwise} {path} slotName="otherwise" {scope} />
  {/if}
</div>

<style>
  .show {
    display: contents;
  }
</style>
