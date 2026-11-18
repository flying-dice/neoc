<!--
  @template List — a kit ListSection of rows. Rows are static children, `ForEach` children, or the shorthand
  `source` + `row` template bound to a live source window. A `selection` Binding (client state, a key or an array
  of keys) is shared with the rows; the `empty` slot shows when a loaded source has no rows. When the window is
  shorter than the source's `total`, reaching the end asks the host for the next page (`source.set {range}`).
  Organisms: ListSection. Molecules: ForEach. Atoms: ActivityIndicator, Children.
-->
<script lang="ts">
  import { ActivityIndicator } from '@agentos/ui/atoms'
  import { ListSection } from '@agentos/ui/organisms'
  import Children from '../atoms/Children.svelte'
  import ForEach from '../molecules/ForEach.svelte'
  import { provideList, useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { Json } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const style = $derived(
    oneOf(
      p('style'),
      ['plain', 'inset', 'insetGrouped', 'grouped', 'sidebar'] as const,
      'insetGrouped'
    )
  )
  const label = $derived(common.label ?? str(p('header')))
  const sourceId = $derived(typeof node.props?.source === 'string' ? node.props.source : undefined)
  const data = $derived(sourceId ? view.store.sources[sourceId] : undefined)
  const empty = $derived(node.slots?.empty ?? [])
  const isEmpty = $derived(data?.kind === 'rows' && data.keys.length === 0 && !data.loading)

  // ── selection ──
  const selection = $derived(view.bindingOf(node.props?.selection, scope))
  const mode = $derived(
    oneOf(p('selectionMode'), ['none', 'single', 'multi'] as const, selection ? 'single' : 'none')
  )
  provideList({
    mode: () => mode,
    isSelected: (tag) => {
      const v = selection?.value
      return Array.isArray(v) ? v.includes(tag) : v !== undefined && v === tag
    },
    select: (tag) => {
      if (!selection || mode === 'none') return
      const v = selection.value
      if (mode === 'multi') {
        const keys = Array.isArray(v) ? v : []
        selection.set(
          keys.includes(tag as Json) ? keys.filter((k) => k !== tag) : [...keys, tag as Json]
        )
      } else selection.set(tag as Json)
    }
  })

  // ── windowing: ask for the next page when the end scrolls into view ──
  let sentinel: HTMLElement | undefined = $state()
  const hasMore = $derived(sourceId ? view.sources.hasMore(sourceId) : false)
  $effect(() => {
    if (!sentinel || !hasMore || !sourceId || typeof IntersectionObserver === 'undefined')
      return undefined
    const id = sourceId
    const io = new IntersectionObserver((entries) => {
      if (entries.some((e) => e.isIntersecting)) view.sources.loadMore(id)
    })
    io.observe(sentinel)
    return () => io.disconnect()
  })
</script>

<div
  class="list"
  role="group"
  aria-label={label || undefined}
  data-testid={common.testid}
  style={common.style}
>
  <ListSection
    style={style === 'plain' ? 'plain' : 'insetGrouped'}
    sidebar={style === 'sidebar'}
    header={str(p('header')) || undefined}
    footer={str(p('footer')) || undefined}
  >
    <Children nodes={node.children} {path} {scope} />
    {#if node.slots?.row}<ForEach {node} {path} {scope} />{/if}
    {#if isEmpty}<Children nodes={empty} {path} slotName="empty" {scope} />{/if}
    {#if hasMore}
      <div
        class="more"
        bind:this={sentinel}
        aria-hidden={data?.kind === 'rows' && data.loading ? undefined : 'true'}
      >
        {#if data?.kind === 'rows' && data.loading}<ActivityIndicator label="Loading more" />{/if}
      </div>
    {/if}
  </ListSection>
</div>

<style>
  .list {
    min-width: 0;
  }
  .more {
    display: flex;
    justify-content: center;
    min-height: 1px;
    padding: 8px 0;
  }
  /* The enclosing Stack owns the horizontal inset (ListSection exposes it as a custom property). */
  .list {
    --list-inset: 0px;
  }
</style>
