<!--
  @organism NavigationLink — a row-like control that pushes a route (`to {name, ...params}`) on the navigation stack
  that contains it, on the client with no round trip. In a list it is a row with a chevron; elsewhere a plain
  button. Params are expressions read at the moment of the tap (`row.id` in a row template). Molecules: ListRow.
  Atoms: Button, Children.
-->
<script lang="ts">
  import { Button } from '@vos/ui/atoms'
  import { ListRow } from '@vos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useColumn, useList, useView } from '../context'
  import { commonOf, str } from '../props'
  import type { Expr } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const list = useList()
  const column = useColumn()

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(view.evaluate(node.props?.title, scope)) || common.label || '')
  const rowKey = $derived(scope.rowKey ?? node.key)
  const position = $derived(list ? list.position(rowKey) : {})
  const hasContent = $derived((node.children?.length ?? 0) > 0)

  function go(): void {
    const to = node.props?.to
    const route = (
      typeof to === 'object' && to !== null && !Array.isArray(to) && !('$ref' in to)
        ? to
        : view.evaluate(to as Expr, scope)
    ) as Record<string, Expr> | null
    if (route && typeof route === 'object') view.navigate(path, route, scope)
    column?.advance()
  }
</script>

{#snippet custom()}
  <Children nodes={node.children} {path} {scope} />
{/snippet}

{#if list}
  <ListRow
    testid={common.testid}
    {title}
    chevron
    disabled={common.disabled}
    onclick={go}
    children={hasContent ? custom : undefined}
    role="listitem"
    {rowKey}
    posinset={position.pos}
    setsize={position.size}
    label={common.label}
  />
{:else}
  <Button
    testid={common.testid}
    variant="plain"
    aria-label={common.label}
    disabled={common.disabled}
    onclick={go}
  >
    {#if hasContent}<Children nodes={node.children} {path} {scope} />{:else}{title}{/if}
  </Button>
{/if}
