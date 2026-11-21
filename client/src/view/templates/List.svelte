<!--
  @template List — a collection of rows in one of five styles (plain, inset, insetGrouped, grouped, sidebar):
  static rows and `Section`s, `ForEach` children, or the shorthand `source` + `row` template bound to a live source.

   - Semantics: a group of lists (a `listbox` of `option`s when the list has a selection), `aria-posinset` /
     `aria-setsize` from the source's total, one tab stop with arrow keys, Home / End / Page keys, type to select.
   - Selection: a `selection` Binding (client state; a key, or an array of keys in `multi`) shared with the rows.
   - Thousands of rows: past 200 rows the list is virtualized: only the rows near the viewport are in the DOM,
     placeholders hold the geometry of rows the host has not sent yet, and scrolling asks the host for the window.
   - Reorder: drag a row's handle, or `Alt+Arrow` on a row, reports `onMove {key, beforeKey}`; the row sits in its
     new place at once and settles when the host answers.
   - Pull to refresh (touch, or the keyboard "Refresh" button, both in the kit list) runs the app's `onRefresh`; the
     spinner shows until it answers. Infinite load widens the source window, or reports `onLoadMore` when `hasMore`.
  Organisms: List, ListSection, VirtualRows. Molecules: FormCell, ListRow. Atoms: ActivityIndicator, Children.
-->
<script lang="ts">
  import {
    covered,
    moveAnnouncement,
    moveBefore,
    moveBy,
    windowFor,
    type RowRange
  } from '@agentos/ui'
  import { ActivityIndicator } from '@agentos/ui/atoms'
  import { ListRow } from '@agentos/ui/molecules'
  import { List as KitList, ListSection, VirtualRows } from '@agentos/ui/organisms'
  import Children from '../atoms/Children.svelte'
  import FormCell from '../molecules/FormCell.svelte'
  import { provideList, useView } from '../context'
  import { childPath } from '../core/tree'
  import type { RowsData } from '../core/store'
  import { commonOf, oneOf, str } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { Json, Node, RowKey } from '../protocol'
  import type { AdapterProps } from '../registry'

  interface Props extends AdapterProps {
    /** `form`: an inset-grouped list of control rows (the `Form` component). */
    variant?: 'form'
  }

  let { node, path, scope, variant }: Props = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const VIRTUALIZE_AT = 200

  const common = $derived(commonOf(ctx, node, path, scope))
  const style = $derived(
    variant === 'form'
      ? 'insetGrouped'
      : oneOf(
          p('style'),
          ['plain', 'inset', 'insetGrouped', 'grouped', 'sidebar'] as const,
          'plain'
        )
  )
  const label = $derived(common.label ?? (str(p('header')) || undefined))

  // ── what the list holds: static nodes, and row sources (ForEach children or the shorthand) ──
  interface Rows {
    kind: 'rows'
    source: string
    template: Node[]
    /** The path rows are addressed under (the ForEach, or the list for the shorthand). */
    at: string
  }
  type Segment = { kind: 'node'; node: Node } | Rows

  const segments = $derived.by(() => {
    const out: Segment[] = []
    for (const child of node.children ?? []) {
      const source = child.type === 'ForEach' ? child.props?.source : undefined
      if (typeof source === 'string')
        out.push({
          kind: 'rows',
          source,
          template: child.slots?.row ?? [],
          at: childPath(path, child.key)
        })
      else out.push({ kind: 'node', node: child })
    }
    const source = node.props?.source
    if (typeof source === 'string' && node.slots?.row)
      out.push({ kind: 'rows', source, template: node.slots.row, at: path })
    return out
  })

  // Rows between Sections share one implicit section (no header); a Section renders itself.
  type Block = { kind: 'section'; node: Node } | { kind: 'loose'; id: number; segments: Segment[] }
  const blocks = $derived.by(() => {
    const out: Block[] = []
    for (const seg of segments) {
      if (seg.kind === 'node' && seg.node.type === 'Section')
        out.push({ kind: 'section', node: seg.node })
      else {
        const last = out.at(-1)
        if (last?.kind === 'loose') last.segments.push(seg)
        else out.push({ kind: 'loose', id: out.length, segments: [seg] })
      }
    }
    return out
  })

  const main = $derived(segments.find((s): s is Rows => s.kind === 'rows'))
  const dataOf = (id: string): RowsData | undefined => {
    const d = view.store.sources[id]
    return d?.kind === 'rows' ? d : undefined
  }
  const mainData = $derived(main ? dataOf(main.source) : undefined)
  const virtualized = $derived(mainData !== undefined && mainData.total > VIRTUALIZE_AT)
  const empty = $derived(
    mainData !== undefined &&
      mainData.total === 0 &&
      !mainData.loading &&
      segments.every((s) => s.kind === 'rows')
  )
  const loading = $derived(mainData?.loading === true)

  // ── selection ──
  const selection = $derived(view.bindingOf(node.props?.selection, scope))
  const mode = $derived(
    variant === 'form'
      ? 'none'
      : oneOf(
          p('selectionMode'),
          ['none', 'single', 'multi'] as const,
          selection ? 'single' : 'none'
        )
  )
  const keysOf = (v: unknown): Json[] =>
    Array.isArray(v) ? (v as Json[]) : v == null ? [] : [v as Json]

  // ── order: the host's, or the one a move left until the host answers ──
  let pending: RowKey[] | null = $state(null)
  let settle: ReturnType<typeof setTimeout> | undefined
  const order = $derived(pending ?? mainData?.keys ?? [])
  $effect(() => {
    void mainData?.keys // the host sent a new order: it is the truth now
    pending = null
  })

  // ── positions (aria-posinset / aria-setsize) ──
  const positions = $derived.by(() => {
    const map: Record<string, number> = {}
    if (mainData) order.forEach((k, i) => (map[String(k)] = mainData.from + i + 1))
    else {
      let n = 0
      const visit = (nodes: Node[]): void => {
        for (const c of nodes)
          if (c.type === 'Row' || c.type === 'NavigationLink') map[c.key] = ++n
          else if (c.type === 'Section') visit(c.children ?? [])
      }
      visit(node.children ?? [])
    }
    return map
  })
  const setSize = $derived(mainData ? mainData.total : Object.keys(positions).length)

  // ── reorder ──
  const canMove = $derived(node.on?.move !== undefined)
  const editing = $derived(view.bindingOf(node.props?.editing, scope))
  const handles = $derived(canMove && (editing ? editing.value === true : true))
  let announcement = $state('')

  const staticKeys = $derived(Object.keys(positions).sort((a, b) => positions[a] - positions[b]))
  function titleOf(key: RowKey): string {
    const row = mainData?.rows[String(key)]
    return str(row?.title ?? row?.name ?? key)
  }
  function commit(key: RowKey, beforeKey: RowKey | null): void {
    const keys: RowKey[] = mainData ? [...order] : staticKeys
    const move = moveBefore(keys, key, beforeKey)
    if (!move) return
    if (mainData) {
      pending = move.keys
      clearTimeout(settle)
      settle = setTimeout(() => (pending = null), 500) // no new order from the host: the row goes back
    }
    announcement = moveAnnouncement(titleOf(key), (mainData?.from ?? 0) + move.to, setSize)
    void view.fire(node, path, 'move', scope, { key, ...(beforeKey === null ? {} : { beforeKey }) })
  }
  const keyOfString = (s: string): RowKey | undefined =>
    (mainData ? order : staticKeys).find((k) => String(k) === s)

  let renderedEnd = 0
  function reorder(key: string, delta: -1 | 1): void {
    const k = keyOfString(key)
    if (k === undefined) return
    const move = moveBy<RowKey>(mainData ? [...order] : staticKeys, k, delta)
    if (move) commit(k, move.beforeKey)
  }
  function dropped(key: string, before: string | null): void {
    const k = keyOfString(key)
    if (k === undefined) return
    if (before !== null) return commit(k, keyOfString(before) ?? null)
    // Past the last rendered row: in front of the next row the list has, if the window ends before the list does.
    const next = mainData && virtualized ? order[renderedEnd - mainData.from] : undefined
    commit(k, next ?? null)
  }

  // ── windowing ──
  let virtual: ReturnType<typeof VirtualRows> | undefined = $state()
  function request(seg: Rows, data: RowsData, range: RowRange): void {
    renderedEnd = range.end
    if (covered(range, { from: data.from, count: data.keys.length })) return
    const win = windowFor(range, data.total, view.sources.page(seg.source))
    view.sources.setRange(seg.source, win.from, win.to)
  }
  function edge(to: 'start' | 'end'): boolean {
    if (!virtual || !mainData) return false
    virtual.scrollToIndex(to === 'start' ? 0 : mainData.total - 1, to === 'start' ? 'start' : 'end')
    return true
  }

  // ── infinite load: the source widens, or the app pages (`hasMore` + `onLoadMore`) ──
  let sentinel: HTMLElement | undefined = $state()
  const widen = $derived(main && !virtualized ? view.sources.hasMore(main.source) : false)
  const paged = $derived(p('hasMore') === true && node.on?.loadMore !== undefined)
  let askedAt = ''
  $effect(() => {
    if (!sentinel || !(widen || paged) || typeof IntersectionObserver === 'undefined')
      return undefined
    const io = new IntersectionObserver((entries) => {
      if (!entries.some((e) => e.isIntersecting)) return
      if (widen && main) view.sources.loadMore(main.source)
      else if (paged) {
        const mark = `${setSize}:${node.children?.length ?? 0}`
        if (askedAt === mark) return // one request per growth of the list
        askedAt = mark
        void view.fire(node, path, 'loadMore', scope)
      }
    })
    io.observe(sentinel)
    return () => io.disconnect()
  })

  // ── pull to refresh: the kit list runs the gesture and the spinner; the app's answer ends it ──
  const refreshable = $derived(node.on?.refresh !== undefined)

  // ── what rows see ──
  provideList({
    mode: () => mode,
    isSelected: (tag) => {
      const v = selection?.value
      return Array.isArray(v) ? v.includes(tag) : v !== undefined && v !== null && v === tag
    },
    select: (tag) => {
      if (!selection || mode === 'none') return
      const current = selection.value
      let next: Json
      if (mode === 'multi') {
        const keys = keysOf(current)
        next = keys.includes(tag as Json) ? keys.filter((k) => k !== tag) : [...keys, tag as Json]
      } else next = tag as Json
      selection.set(next)
      void view.fire(node, path, 'selectionChange', scope, { keys: keysOf(next) })
    },
    tap: (key) => void view.fire(node, path, 'tap', scope, { key }),
    handlesTap: () => node.on?.tap !== undefined,
    position: (key) => {
      const pos = positions[String(key)]
      return pos === undefined ? {} : { pos, size: setSize }
    },
    reorderable: () => handles,
    cells: () => variant === 'form'
  })
</script>

{#snippet plainRows(seg: Rows)}
  {@const data = dataOf(seg.source)}
  {@const keys = seg === main ? order : (data?.keys ?? [])}
  {@const groupBy = seg === main ? str(p('groupBy')) : ''}
  {#if data}
    {#each keys as key, i (key)}
      {@const row = data.rows[String(key)]}
      {#if row}
        {#if groupBy && (i === 0 || String(data.rows[String(keys[i - 1])]?.[groupBy]) !== String(row[groupBy]))}
          <div class="group-head" role="presentation">{str(row[groupBy])}</div>
        {/if}
        <Children
          nodes={seg.template}
          path={seg.at}
          slotName="row"
          scope={{ ...scope, row, rowKey: key, source: seg.source }}
        />
      {/if}
    {/each}
  {/if}
{/snippet}

{#snippet windowed(seg: Rows, data: RowsData)}
  <VirtualRows
    testid="{common.testid}-virtual"
    total={data.total}
    keys={data.keys}
    from={data.from}
    onrange={(range) => request(seg, data, range)}
    bind:this={virtual}
  >
    {#snippet row(index: number)}
      {@const at = index - data.from}
      {@const key = at >= 0 && at < order.length ? order[at] : undefined}
      {@const loaded = key === undefined ? undefined : data.rows[String(key)]}
      {#if key !== undefined && loaded}
        <Children
          nodes={seg.template}
          path={seg.at}
          slotName="row"
          scope={{ ...scope, row: loaded, rowKey: key, source: seg.source }}
        />
      {:else}
        <ListRow
          testid="{common.testid}-placeholder-{index}"
          title=""
          role="listitem"
          placeholder
          posinset={index + 1}
          setsize={data.total}
        />
      {/if}
    {/snippet}
  </VirtualRows>
{/snippet}

{#snippet segment(seg: Segment)}
  {#if seg.kind === 'node'}
    {#if variant === 'form'}
      <FormCell node={seg.node} {path} {scope} />
    {:else}
      <Children nodes={[seg.node]} {path} {scope} />
    {/if}
  {:else}
    {@const data = dataOf(seg.source)}
    {#if seg === main && virtualized && data}
      {@render windowed(seg, data)}
    {:else}
      {@render plainRows(seg)}
    {/if}
  {/if}
{/snippet}

<div
  class="list"
  role={variant === 'form' && label ? 'form' : undefined}
  aria-label={variant === 'form' ? label : undefined}
  style={joinStyle(common.style, surfaceStyle(ctx, node, scope))}
  aria-busy={loading || undefined}
>
  <Children nodes={node.slots?.header} {path} slotName="header" {scope} />
  <KitList
    testid={common.testid}
    {style}
    selection={mode}
    {label}
    reorderable={handles}
    onreorder={reorder}
    onmove={dropped}
    onedge={edge}
    css={p('separators') === false ? '--list-separator: transparent' : undefined}
    onrefresh={refreshable ? () => view.fire(node, path, 'refresh', scope) : undefined}
    {announcement}
  >
    {#each blocks as block (block.kind === 'section' ? block.node.key : `loose-${block.id}`)}
      {#if block.kind === 'section'}
        <Children nodes={[block.node]} {path} {scope} />
      {:else}
        <ListSection>
          {#each block.segments as seg, i (seg.kind === 'node' ? seg.node.key : `rows-${seg.source}-${i}`)}
            {@render segment(seg)}
          {/each}
          {#if empty}<Children nodes={node.slots?.empty} {path} slotName="empty" {scope} />{/if}
          {#if widen || paged}
            <div class="more" bind:this={sentinel} aria-hidden={loading ? undefined : 'true'}>
              {#if loading}<ActivityIndicator label="Loading more" />{/if}
            </div>
          {/if}
        </ListSection>
      {/if}
    {/each}
  </KitList>
  <Children nodes={node.slots?.footer} {path} slotName="footer" {scope} />
</div>

<style>
  .list {
    min-width: 0;
  }
  /* `groupBy`: a header where the grouping field changes (rows arrive ordered by it). */
  .group-head {
    padding: 12px var(--row-inset) 6px;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    text-transform: uppercase;
    color: var(--label-secondary);
    background: var(--list-row-bg, var(--bg-grouped-elevated-secondary));
  }
  .more {
    display: flex;
    justify-content: center;
    min-height: 1px;
    padding: 8px 0;
  }
</style>
