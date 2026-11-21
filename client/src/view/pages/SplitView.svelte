<!--
  @page SplitView — two or three columns (`sidebar`, `content`, `detail`). Sidebars are 280 to 320 px
  (`sidebarWidth` narrow / regular / wide). Below 700 px the split collapses to one column and becomes a navigation
  flow, sidebar → content → detail: choosing a row moves it forward, the screen's back button (named after the
  previous column's title) moves it back, `preferredCompact` picks the column shown first. In a narrow regular
  window a three-column split hides the sidebar first. `visibility` (`all`, `double`, `detail`) is a Binding to client
  state; screens beside a hidden sidebar get a button to show it. The sidebar is a `navigation` landmark, the last
  column `main`. Templates: SplitViewTemplate. Atoms: NavLevel, Children.
-->
<script lang="ts">
  import { SplitViewTemplate } from '@vos/ui/templates'
  import { untrack } from 'svelte'
  import Children from '../atoms/Children.svelte'
  import NavLevel from '../atoms/NavLevel.svelte'
  import { useView, type NavLevel as Level } from '../context'
  import { commonOf, oneOf } from '../props'
  import type { AdapterProps } from '../registry'

  type Column = 'sidebar' | 'content' | 'detail'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const hasContentSlot = $derived((node.slots?.content?.length ?? 0) > 0)
  const columns = $derived(p('columns') === 3 ? 3 : p('columns') === 2 ? 2 : hasContentSlot ? 3 : 2)
  const order = $derived<Column[]>(
    columns === 3 ? ['sidebar', 'content', 'detail'] : ['sidebar', 'detail']
  )
  const sidebarWidth = $derived(
    { narrow: 280, regular: 300, wide: 320 }[
      oneOf(p('sidebarWidth'), ['narrow', 'regular', 'wide'] as const, 'regular')
    ]
  )

  // visibility: client state when bound, else local
  const binding = $derived(view.bindingOf(node.props?.visibility, scope))
  let localVisibility = $state<'all' | 'double' | 'detail'>('all')
  const visibility = $derived(
    oneOf(binding ? binding.value : localVisibility, ['all', 'double', 'detail'] as const, 'all')
  )
  function setVisibility(next: 'all' | 'double' | 'detail'): void {
    if (binding) binding.set(next)
    else localVisibility = next
  }

  let collapsed = $state(false)
  let effective = $state<'all' | 'double' | 'detail'>('all')

  // ── the collapsed flow ──
  let compact = $state<Column>('sidebar')
  let seeded = false
  $effect(() => {
    if (seeded) return
    seeded = true
    const wanted = oneOf(
      p('preferredCompact'),
      ['sidebar', 'content', 'detail'] as const,
      'sidebar'
    )
    compact = order.includes(wanted) ? wanted : 'sidebar'
  })
  // An app that sets `visibility` while collapsed moves the flow too (`detail` shows the detail, `double` the content).
  let lastVisibility = untrack(() => visibility)
  $effect(() => {
    const v = visibility
    if (v === lastVisibility) return
    lastVisibility = v
    compact = v === 'detail' ? 'detail' : v === 'double' && columns === 3 ? 'content' : 'sidebar'
  })

  let titles = $state<string[]>([])
  // Live objects: their getters read the current column order, collapse state and visibility.
  const levelFor = (col: Column): Level => ({
    get depth() {
      return order.indexOf(col)
    },
    landmark: col === 'sidebar' ? 'navigation' : col === 'detail' ? 'main' : 'region',
    get back() {
      const index = order.indexOf(col)
      return collapsed && index > 0
        ? { label: titles[index - 1], go: () => void (compact = order[index - 1]) }
        : undefined
    },
    get sidebar() {
      return col === 'sidebar' || collapsed
        ? undefined
        : {
            shown: effective === 'all',
            toggle: () =>
              setVisibility(effective === 'all' ? (columns === 3 ? 'double' : 'detail') : 'all')
          }
    },
    setTitle: (title) => void (titles[order.indexOf(col)] = title)
  })
  const levels: Record<Column, Level> = {
    sidebar: levelFor('sidebar'),
    content: levelFor('content'),
    detail: levelFor('detail')
  }
  const advance = (col: Column): { advance(): void } => ({
    advance: () => {
      if (!collapsed) return
      const next = order[order.indexOf(col) + 1]
      if (next) compact = next
    }
  })
</script>

{#snippet sidebar()}
  <NavLevel level={levels.sidebar} column={advance('sidebar')}>
    <Children nodes={node.slots?.sidebar} {path} slotName="sidebar" {scope} />
  </NavLevel>
{/snippet}
{#snippet content()}
  <NavLevel level={levels.content} column={advance('content')}>
    <Children nodes={node.slots?.content} {path} slotName="content" {scope} />
  </NavLevel>
{/snippet}
{#snippet detail()}
  <NavLevel level={levels.detail}>
    <Children nodes={node.slots?.detail} {path} slotName="detail" {scope} />
  </NavLevel>
{/snippet}

<div class="host" style={common.style}>
  <SplitViewTemplate
    testid={common.testid}
    {sidebarWidth}
    collapseAt={700}
    {visibility}
    compactColumn={compact}
    prominentDetail={p('style') === 'prominentDetail'}
    keepMounted
    bind:collapsed
    bind:effective
    {sidebar}
    content={columns === 3 ? content : undefined}
    {detail}
  />
</div>

<style>
  .host {
    display: flex;
    flex: 1;
    min-height: 0;
    min-width: 0;
    height: 100%;
  }
</style>
