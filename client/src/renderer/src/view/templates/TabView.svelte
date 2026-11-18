<!--
  @template TabView — tabs and their panels. Each child is a tab: its `tag` (else its key) is the value `selection`
  (a Binding to client state) holds, and its `tabItem` slot names it (a `Label`, `Text` or `Icon` with `title` /
  `text` / `label`, an `icon`, and a `Badge` for the count). Styles `bottom`, `top`, `sidebar` and `page`; a
  `sidebar` TabView becomes `bottom` in a compact window. Organisms: TabView. Atoms: Children.
-->
<script lang="ts">
  import { icons, type IconName } from '@agentos/ui'
  import { TabView as KitTabView } from '@agentos/ui/organisms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { Json, Node } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const requested = $derived(
    oneOf(p('style'), ['bottom', 'top', 'sidebar', 'page'] as const, 'bottom')
  )
  const style = $derived(
    requested === 'sidebar' && view.store.env.sizeClass === 'compact' ? 'bottom' : requested
  )

  /** A tab's name, icon and badge, read from the nodes of its `tabItem` slot (they are data here, not rendered). */
  function describe(tab: Node): {
    label: string
    icon: IconName | undefined
    badge: number | string | undefined
  } {
    let label = str(
      view.evaluate(tab.props?.label, scope) ?? view.evaluate(tab.props?.title, scope)
    )
    let icon: IconName | undefined
    let badge: number | string | undefined
    for (const item of tab.slots?.tabItem ?? []) {
      const get = (name: string): unknown => view.evaluate(item.props?.[name], scope)
      if (item.type === 'Badge') {
        const v = get('value')
        if (typeof v === 'number' || typeof v === 'string') badge = v
        continue
      }
      const text = get('title') ?? get('text') ?? get('label')
      if (text !== undefined && text !== null && !label) label = str(text)
      const glyph = get('icon') ?? get('name')
      if (typeof glyph === 'string' && glyph in icons) icon = glyph as IconName
    }
    return { label: label || tab.key, icon, badge }
  }

  const tabs = $derived(
    (node.children ?? []).map((tab) => {
      const tag = view.evaluate(tab.props?.tag, scope)
      return {
        node: tab,
        tag: (tag ?? tab.key) as Json,
        id: String(tag ?? tab.key),
        ...describe(tab)
      }
    })
  )

  const selection = $derived(view.bindingOf(node.props?.selection, scope))
  let local = $state<string | undefined>()
  const selected = $derived(selection ? String(selection.value ?? '') : local)

  function select(id: string): void {
    const tab = tabs.find((t) => t.id === id)
    if (!tab) return
    if (selection) selection.set(tab.tag)
    else local = id
  }
</script>

{#snippet panel(id: string)}
  {@const tab = tabs.find((t) => t.id === id)}
  {#if tab}<Children nodes={[tab.node]} {path} {scope} />{/if}
{/snippet}

<KitTabView
  testid={common.testid}
  tabs={tabs.map(({ id, label, icon, badge }) => ({ id, label, icon, badge }))}
  {selected}
  {style}
  label={common.label ?? 'Tabs'}
  onselect={select}
  {panel}
  css={common.style}
/>
