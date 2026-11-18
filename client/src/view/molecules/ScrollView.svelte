<!--
  @molecule ScrollView — scrolls its content: `axis`, `indicators`, `snap`, pull to refresh (`refresh` +
  `onRefresh`, the spinner runs until the app answers), `scrollTo` (a Binding to the key of a node to bring into
  view) and `offset` (a Binding written with the scroll position). `onScroll {x, y}` is throttled to 10 per second.
  Atoms: ScrollView, Children.
-->
<script lang="ts">
  import { ScrollView } from '@agentos/ui/atoms'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { findByKey } from '../core/tree'
  import { testid as testidOf } from '../core/testid'
  import { commonOf, oneOf, str } from '../props'
  import { joinStyle, surfaceStyle } from '../style'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const scrollTo = $derived(view.bindingOf(node.props?.scrollTo, scope))
  const offset = $derived(view.bindingOf(node.props?.offset, scope))
  let element: HTMLElement | undefined = $state()

  // The app (or an action) names a node by key; bring it into view, then clear the request.
  $effect(() => {
    const key = scrollTo?.value
    if (typeof key !== 'string' || !key || !element || !view.store.tree) return
    const hit = findByKey(view.store.tree, key)
    const el = hit
      ? element.querySelector<HTMLElement>(
          `[data-testid="${CSS.escape(testidOf(ctx.app, hit.path))}"]`
        )
      : null
    el?.scrollIntoView({ block: 'nearest' })
    scrollTo?.set(null)
  })

  let last = 0
  function scrolled(pos: { x: number; y: number }): void {
    offset?.set(pos)
    const now = Date.now()
    if (node.on?.scroll && now - last >= 100) {
      last = now
      void view.fire(node, path, 'scroll', scope, pos)
    }
  }
</script>

<ScrollView
  testid={common.testid}
  axis={oneOf(p('axis'), ['v', 'h', 'both'] as const, 'v')}
  indicators={p('indicators') !== false}
  snap={p('snap') === true}
  label={common.label ?? 'Scrollable content'}
  refresh={p('refresh') === true && node.on?.refresh !== undefined}
  onrefresh={() => view.fire(node, path, 'refresh', scope)}
  onscroll={scrolled}
  bind:element
  style={joinStyle(
    common.style,
    surfaceStyle(ctx, node, scope),
    str(p('axis')) === 'h' ? undefined : 'flex:1 1 auto'
  )}
>
  <Children nodes={node.children} {path} {scope} />
</ScrollView>
