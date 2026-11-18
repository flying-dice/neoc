<!--
  @page Screen — one navigable screen: the kit ScreenTemplate with a NavigationBar (title, large or inline,
  back button) and a toolbar filled from the `toolbar.*` slots. `searchable.text` is a Binding to client state: the
  search field writes it with no round trip, and sources that read it re-query. The screen is the window's `main`
  landmark. Templates: ScreenTemplate. Organisms: NavigationBar. Molecules: SearchField. Atoms: Children.
-->
<script lang="ts">
  import { SearchField } from '@agentos/ui/molecules'
  import { NavigationBar } from '@agentos/ui/organisms'
  import { ScreenTemplate } from '@agentos/ui/templates'
  import Children from '../atoms/Children.svelte'
  import { useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { Node as NodeType } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const display = $derived(oneOf(p('titleDisplay'), ['large', 'inline', 'auto'] as const, 'auto'))
  const large = $derived(
    display === 'large' || (display === 'auto' && ctx.view.store.env.sizeClass === 'regular')
  )
  const backLabel = $derived(str(p('backLabel')) || undefined)
  const background = $derived(oneOf(p('background'), ['plain', 'grouped'] as const, 'grouped'))

  /** `searchable {text: Binding, placeholder}`: the binding stays a raw ref (it is written, not evaluated). */
  const searchable = $derived(node.props?.searchable as Record<string, unknown> | undefined)
  const search = $derived(view.bindingOf(searchable?.text as never, scope))
  const searchPlaceholder = $derived(
    str(view.evaluate(searchable?.placeholder as never, scope), 'Search')
  )

  const toolbar = (name: string): NodeType[] => node.slots?.[`toolbar.${name}`] ?? []
  const leading = $derived(toolbar('leading'))
  const trailing = $derived([
    ...toolbar('primaryAction'),
    ...toolbar('secondaryAction'),
    ...toolbar('trailing')
  ])
  const principal = $derived(toolbar('principal'))
  const bottom = $derived(toolbar('bottom'))
</script>

{#snippet bar()}
  <NavigationBar
    testid="{common.testid}-navbar"
    {title}
    {large}
    {backLabel}
    onback={() => view.fire(node, path, 'back', scope)}
    leading={leading.length ? leadingContent : undefined}
    trailing={trailing.length ? trailingContent : undefined}
    accessory={search || principal.length ? accessory : undefined}
  />
{/snippet}
{#snippet leadingContent()}
  <Children nodes={leading} {path} slotName="toolbar.leading" {scope} />
{/snippet}
{#snippet trailingContent()}
  <Children
    nodes={node.slots?.['toolbar.primaryAction']}
    {path}
    slotName="toolbar.primaryAction"
    {scope}
  />
  <Children
    nodes={node.slots?.['toolbar.secondaryAction']}
    {path}
    slotName="toolbar.secondaryAction"
    {scope}
  />
  <Children nodes={node.slots?.['toolbar.trailing']} {path} slotName="toolbar.trailing" {scope} />
{/snippet}
{#snippet accessory()}
  {#if search}
    <SearchField
      testid="{common.testid}-search"
      placeholder={searchPlaceholder}
      label={searchPlaceholder}
      bind:value={() => str(search.value), (v) => search.set(v)}
    />
  {/if}
  {#if principal.length}<Children
      nodes={principal}
      {path}
      slotName="toolbar.principal"
      {scope}
    />{/if}
{/snippet}
{#snippet content()}
  <main class="screen" style={common.style}>
    <Children nodes={node.children} {path} {scope} />
  </main>
{/snippet}
{#snippet bottomBar()}
  <Children nodes={bottom} {path} slotName="toolbar.bottom" {scope} />
{/snippet}

<ScreenTemplate
  {background}
  navigationBar={bar}
  {content}
  bottomBar={bottom.length ? bottomBar : undefined}
/>

<style>
  .screen {
    display: block;
    min-width: 0;
  }
</style>
