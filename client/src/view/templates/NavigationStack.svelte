<!--
  @template NavigationStack — a push / pop stack of screens. The path (`{name, ...params}` routes) is client state: a
  `path` Binding to a declared client entry (so it can persist), or the stack's own when unbound. The `root` slot is
  the first screen; a route renders the `dest:<name>` slot with `route.*` in scope. `nav.push`, `nav.pop` and
  `nav.popToRoot` actions, and `NavigationLink`, reach the innermost stack containing them, with no round trip. Each
  screen gets its back button (labelled with the previous screen's title) through its `NavLevel`. Organisms:
  NavigationStack. Atoms: NavLevel, Children.
-->
<script lang="ts">
  import { NavigationStack as Stack } from '@vos/ui/organisms'
  import { untrack } from 'svelte'
  import Children from '../atoms/Children.svelte'
  import NavLevel from '../atoms/NavLevel.svelte'
  import { useNavLevel, useView, type NavLevel as Level } from '../context'
  import { asPath, popRoute, pushRoute, reconcile, removeEntry, type StackEntry } from '../core/nav'
  import { commonOf } from '../props'
  import type { Json, Route } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const parent = useNavLevel()

  const common = $derived(commonOf(ctx, node, path, scope))
  const binding = $derived(view.bindingOf(node.props?.path, scope))
  let local = $state<Route[]>([])
  const routes = $derived(asPath(binding ? binding.value : local))

  let counter = 0
  const nextId = (): string => `n${++counter}`
  let entries = $state<StackEntry[]>(
    reconcile(
      [],
      untrack(() => routes),
      nextId
    )
  )
  $effect(() => {
    const next = routes
    entries = reconcile(
      untrack(() => entries),
      next,
      nextId
    )
  })

  function setRoutes(next: Route[]): void {
    if (binding) binding.set(next as unknown as Json)
    else local = next
    void view.fire(node, path, 'pathChange', scope, { path: next as unknown as Json })
  }

  $effect(() =>
    view.nav.register({
      key: node.key,
      path,
      push: (route) => setRoutes(pushRoute(routes, route)),
      pop: () => routes.length && setRoutes(popRoute(routes)),
      popToRoot: () => routes.length && setRoutes([])
    })
  )

  // ── levels: where each screen sits, and the title the next one names its back button with ──
  let titles = $state<string[]>([])
  const levels: Record<string, Level> = {}
  function levelOf(entry: StackEntry): Level {
    let level: Level | undefined = levels[entry.id]
    if (!level) {
      const depth = entries.filter((e) => !e.leaving).findIndex((e) => e.id === entry.id)
      level = {
        depth,
        landmark: parent?.landmark ?? 'main',
        get back() {
          return depth > 0
            ? {
                label: titles[depth - 1],
                go: () => {
                  if (routes.length) setRoutes(popRoute(routes))
                }
              }
            : parent?.back
        },
        get sidebar() {
          return depth === 0 ? parent?.sidebar : undefined
        },
        setTitle: (title) => void (titles[depth] = title)
      }
      levels[entry.id] = level
    }
    return level
  }

  const items = $derived(entries.map((e) => ({ id: e.id, leaving: e.leaving })))
  const slotOf = (entry: StackEntry): string =>
    entry.route === null ? 'root' : `dest:${entry.route.name}`
</script>

{#snippet screen(id: string)}
  {@const entry = entries.find((e) => e.id === id)}
  {#if entry}
    <NavLevel level={levelOf(entry)}>
      <Children
        nodes={node.slots?.[slotOf(entry)]}
        {path}
        slotName={slotOf(entry)}
        scope={entry.route === null ? scope : { ...scope, route: entry.route }}
      />
    </NavLevel>
  {/if}
{/snippet}

<div class="host" style={common.style}>
  <Stack
    testid={common.testid}
    {items}
    {screen}
    onleft={(id) => {
      entries = removeEntry(entries, id)
      delete levels[id]
    }}
  />
</div>

<style>
  .host {
    display: flex;
    flex-direction: column;
    flex: 1;
    min-height: 0;
    height: 100%;
    min-width: 0;
  }
</style>
