<!--
  @molecule ForEach — instantiates its row template (slot "row") once per key of a bound source window. The tree
  carries the template once; thousands of rows cost data, not nodes. Each instance sees `row.*` for its
  row and is addressed `path/@row/<template key>` plus the row key. Atoms: Children.
-->
<script lang="ts">
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()

  const sourceId = $derived(str(ctx.view.evaluate(node.props?.source, scope)))
  const data = $derived(ctx.view.store.sources[sourceId])
  const template = $derived(node.slots?.row ?? [])
</script>

{#if data?.kind === 'rows'}
  {#each data.keys as key (key)}
    {@const row = data.rows[String(key)]}
    {#if row}
      <Children
        nodes={template}
        {path}
        slotName="row"
        scope={{ row, rowKey: key, source: sourceId }}
      />
    {/if}
  {/each}
{/if}
