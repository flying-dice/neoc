<!--
  @organism Row — a list row: the kit ListRow with wire props. Inside a `List` it is a managed row: a `listitem`
  with an invisible full-row tap button (so interactive `trailing` content is a sibling, never a nested button), or an
  `option` when the list has a selection. A tap selects the row's `tag` (client state, no round trip), runs the
  node's `tap` handler and the list's `onTap {key}`. `swipeLeading` / `swipeTrailing` hold Buttons: they become swipe
  actions, also reachable from the keyboard (`Alt+Arrow`, the menu key) and by screen readers. Molecules: ListRow,
  SwipeAction. Atoms: Children.
-->
<script lang="ts">
  import { icons, type IconName } from '@vos/ui'
  import { ListRow, SwipeAction } from '@vos/ui/molecules'
  import Children from '../atoms/Children.svelte'
  import { useColumn, useList, useView } from '../context'
  import { childPath } from '../core/tree'
  import { testid as testidOf } from '../core/testid'
  import { commonOf, oneOf, str } from '../props'
  import type { Node } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const list = useList()
  const column = useColumn()
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
  const iconColor = $derived(
    typeof p('iconTint') === 'string' ? `var(--ios-${p('iconTint') as string})` : undefined
  )
  const role = $derived(oneOf(p('role'), ['normal', 'destructive', 'action'] as const, 'normal'))
  const rowKey = $derived(scope.rowKey ?? node.key)
  /** What a selection list stores for this row: its `tag`, else the row's key. */
  const tag = $derived(p('tag') ?? scope.rowKey)
  const selected = $derived(list ? list.isSelected(tag) : false)
  const selectable = $derived(list !== undefined && list.mode() !== 'none' && tag !== undefined)
  const hasLeading = $derived((node.slots?.leading?.length ?? 0) > 0)
  const hasTrailing = $derived((node.slots?.trailing?.length ?? 0) > 0)
  const hasContent = $derived((node.children?.length ?? 0) > 0)
  const managedRole = $derived(list ? (selectable ? 'option' : 'listitem') : undefined)
  const position = $derived(list ? list.position(rowKey) : {})
  // Outside a list the row is a plain button, which cannot hold interactive trailing content: it is then not tappable.
  const tappable = $derived(
    !common.disabled &&
      (list
        ? selectable || node.on?.tap !== undefined || list.handlesTap()
        : !hasTrailing && node.on?.tap !== undefined)
  )
  const multi = $derived(list?.mode() === 'multi' && selectable)

  function tap(): void {
    if (selectable) list?.select(tag)
    void view.fire(node, path, 'tap', scope)
    list?.tap(rowKey)
    column?.advance()
  }

  // ── swipe actions ──
  interface Action {
    node: Node
    path: string
    testid: string
    title: string
    icon: IconName | undefined
    role: 'destructive' | 'normal' | 'neutral'
    full: boolean
  }
  const actions = (slot: 'swipeLeading' | 'swipeTrailing'): Action[] =>
    (node.slots?.[slot] ?? []).map((a) => {
      const ap = childPath(path, a.key, slot)
      const r = view.evaluate(a.props?.role, scope)
      const icon = view.evaluate(a.props?.icon, scope)
      return {
        node: a,
        path: ap,
        testid: testidOf(ctx.app, ap, scope.rowKey, a.props?.testid),
        title: str(view.evaluate(a.props?.title, scope) ?? view.evaluate(a.props?.label, scope)),
        icon: typeof icon === 'string' && icon in icons ? (icon as IconName) : undefined,
        role: r === 'destructive' ? 'destructive' : r === 'cancel' ? 'neutral' : 'normal',
        full: view.evaluate(a.props?.fullSwipe, scope) === true
      }
    })
  const leadingActions = $derived(actions('swipeLeading'))
  const trailingActions = $derived(actions('swipeTrailing'))
  let open: 'none' | 'leading' | 'trailing' = $state('none')

  function act(a: Action): void {
    open = 'none'
    void view.fire(a.node, a.path, 'tap', scope)
  }
</script>

{#snippet leadingContent()}
  <Children nodes={node.slots?.leading} {path} slotName="leading" {scope} />
{/snippet}
{#snippet trailingContent()}
  <Children nodes={node.slots?.trailing} {path} slotName="trailing" {scope} />
{/snippet}
{#snippet custom()}
  <Children nodes={node.children} {path} {scope} />
{/snippet}
{#snippet swipeLeadingContent()}
  {#each leadingActions as a (a.node.key)}
    <SwipeAction
      testid={a.testid}
      title={a.title}
      icon={a.icon}
      role={a.role}
      onclick={() => act(a)}
    />
  {/each}
{/snippet}
{#snippet swipeTrailingContent()}
  {#each trailingActions as a (a.node.key)}
    <SwipeAction
      testid={a.testid}
      title={a.title}
      icon={a.icon}
      role={a.role}
      onclick={() => act(a)}
    />
  {/each}
{/snippet}

<ListRow
  testid={common.testid}
  {title}
  {subtitle}
  {value}
  icon={iconName}
  {iconColor}
  chevron={p('chevron') === true}
  {selected}
  destructive={role === 'destructive'}
  action={role === 'action'}
  disabled={common.disabled}
  onclick={tappable ? tap : undefined}
  leading={hasLeading ? leadingContent : undefined}
  trailing={hasTrailing ? trailingContent : undefined}
  children={hasContent ? custom : undefined}
  role={managedRole}
  {rowKey}
  posinset={position.pos}
  setsize={position.size}
  label={common.label}
  swipeLeading={leadingActions.length ? swipeLeadingContent : undefined}
  swipeTrailing={trailingActions.length ? swipeTrailingContent : undefined}
  fullSwipe={{ leading: leadingActions[0]?.full, trailing: trailingActions[0]?.full }}
  onfullswipe={(side) => {
    const first = (side === 'leading' ? leadingActions : trailingActions)[0]
    if (first) act(first)
  }}
  bind:open
  reorder={list?.reorderable() ?? false}
  check={multi ? selected : undefined}
/>
