<!--
  @organism Row — a list row: the kit ListRow with wire props. Inside a `List` with a selection binding a tap
  selects the row's `tag` (client state, no round trip); `on.tap` also runs the node's handler. `leading` and
  `trailing` slots hold nodes; a row with interactive trailing content is not itself tappable (no nested buttons).
  Molecules: ListRow. Atoms: Children.
-->
<script lang="ts">
  import { icons, type IconName } from '@agentos/ui'
  import { ListRow } from '@agentos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useList, useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const list = useList()
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const subtitle = $derived(str(p('subtitle')) || undefined)
  const badge = $derived(p('badge'))
  const value = $derived(
    str(p('value'), badge === undefined || badge === null ? '' : str(badge)) || undefined
  )
  const iconName = $derived(
    typeof p('icon') === 'string' && (p('icon') as string) in icons
      ? (p('icon') as IconName)
      : undefined
  )
  const role = $derived(oneOf(p('role'), ['normal', 'destructive', 'action'] as const, 'normal'))
  const tag = $derived(p('tag'))
  const selected = $derived(list ? list.isSelected(tag) : false)
  const selectable = $derived(list !== undefined && list.mode() !== 'none' && tag !== undefined)
  const hasLeading = $derived((node.slots?.leading?.length ?? 0) > 0)
  const hasTrailing = $derived((node.slots?.trailing?.length ?? 0) > 0)
  const tappable = $derived(
    !common.disabled && !hasTrailing && (selectable || node.on?.tap !== undefined)
  )

  function tap(): void {
    if (selectable) list?.select(tag)
    view.fire(node, path, 'tap', scope)
  }
</script>

{#snippet leadingContent()}
  <Children nodes={node.slots?.leading} {path} slotName="leading" {scope} />
{/snippet}
{#snippet trailingContent()}
  <Children nodes={node.slots?.trailing} {path} slotName="trailing" {scope} />
{/snippet}

<ListRow
  testid={common.testid}
  {title}
  {subtitle}
  {value}
  icon={iconName}
  chevron={p('chevron') === true}
  {selected}
  destructive={role === 'destructive'}
  action={role === 'action'}
  onclick={tappable ? tap : undefined}
  leading={hasLeading ? leadingContent : undefined}
  trailing={hasTrailing ? trailingContent : undefined}
/>
