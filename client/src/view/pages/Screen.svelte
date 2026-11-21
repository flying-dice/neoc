<!--
  @page Screen — one navigable screen: the kit ScreenTemplate with a NavigationBar (title, large or inline, optional
  subtitle) and a toolbar filled from the `toolbar.*` slots. The iPadOS back button appears when the screen is pushed
  on a navigation stack or is a later column of a collapsed split view, labelled with the previous screen's title
  (`backLabel` overrides it); a button to show a hidden sidebar appears beside one. `searchable.text` is a Binding to
  client state: the field writes it with no round trip, `scopes` add a segmented control, Enter reports
  `onSearchSubmit`. `refresh` + `onRefresh` is pull to refresh. The screen is the `main` landmark, or the `navigation`
  or `region` of a split view column. Templates: ScreenTemplate. Organisms: NavigationBar. Molecules: SearchField,
  IconButton. Atoms: SegmentedControl, Children.
-->
<script lang="ts">
  import { SegmentedControl } from '@vos/ui/atoms'
  import { IconButton, SearchField } from '@vos/ui/molecules'
  import { NavigationBar } from '@vos/ui/organisms'
  import { ScreenTemplate } from '@vos/ui/templates'
  import Children from '../atoms/Children.svelte'
  import { useNavLevel, useView } from '../context'
  import { commonOf, oneOf, str } from '../props'
  import type { Node as NodeType } from '../protocol'
  import type { AdapterProps } from '../registry'

  let { node, path, scope }: AdapterProps = $props()
  const ctx = useView()
  const { view } = ctx
  const level = useNavLevel()
  const p = (name: string): unknown => view.evaluate(node.props?.[name], scope)

  const common = $derived(commonOf(ctx, node, path, scope))
  const title = $derived(str(p('title')))
  const subtitle = $derived(str(p('subtitle')) || undefined)
  const display = $derived(oneOf(p('titleDisplay'), ['large', 'inline', 'auto'] as const, 'auto'))
  const large = $derived(
    display === 'large' || (display === 'auto' && view.store.env.sizeClass === 'regular')
  )
  const back = $derived(level?.back)
  const backLabel = $derived(back ? str(p('backLabel')) || back.label || 'Back' : undefined)
  const background = $derived(oneOf(p('background'), ['plain', 'grouped'] as const, 'grouped'))
  const landmark = $derived(level?.landmark ?? 'main')

  // The next screen names its back button after this one.
  $effect(() => level?.setTitle(title))

  /** `searchable {text: Binding, placeholder, scopes, selectedScope: Binding}`: bindings stay raw refs (they are written). */
  const searchable = $derived(node.props?.searchable as Record<string, unknown> | undefined)
  const search = $derived(view.bindingOf(searchable?.text as never, scope))
  const searchPlaceholder = $derived(
    str(view.evaluate(searchable?.placeholder as never, scope), 'Search')
  )
  const scopes = $derived(
    (view.evaluate(searchable?.scopes as never, scope) as unknown[] | undefined)?.map(String) ?? []
  )
  const scopeBinding = $derived(view.bindingOf(searchable?.selectedScope as never, scope))
  const selectedScope = $derived(str(scopeBinding?.value, scopes[0] ?? ''))

  const toolbar = (name: string): NodeType[] => node.slots?.[`toolbar.${name}`] ?? []
  const leading = $derived(toolbar('leading'))
  const trailing = $derived([
    ...toolbar('primaryAction'),
    ...toolbar('secondaryAction'),
    ...toolbar('trailing')
  ])
  const principal = $derived(toolbar('principal'))
  const bottom = $derived(toolbar('bottom'))
  const sidebar = $derived(level?.sidebar)
  const hasLeading = $derived(leading.length > 0 || sidebar !== undefined)

  const refreshable = $derived(p('refresh') === true && node.on?.refresh !== undefined)

  function searchKey(e: KeyboardEvent): void {
    if (e.key === 'Enter' && node.on?.searchSubmit)
      void view.fire(node, path, 'searchSubmit', scope)
  }
</script>

{#snippet bar()}
  <NavigationBar
    testid="{common.testid}-navbar"
    {title}
    {large}
    {backLabel}
    onback={() => back?.go()}
    leading={hasLeading ? leadingContent : undefined}
    trailing={trailing.length ? trailingContent : undefined}
    accessory={search || principal.length || subtitle ? accessory : undefined}
  />
{/snippet}
{#snippet leadingContent()}
  {#if sidebar}
    <IconButton
      testid="{common.testid}-sidebar-toggle"
      icon="sidebar"
      label={sidebar.shown ? 'Hide sidebar' : 'Show sidebar'}
      size="small"
      onclick={() => sidebar.toggle()}
    />
  {/if}
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
  {#if subtitle}<p class="subtitle">{subtitle}</p>{/if}
  {#if search}
    <!-- svelte-ignore a11y_no_static_element_interactions (Enter in the search field reports onSearchSubmit) -->
    <div class="search" onkeydown={searchKey} data-testid="{common.testid}-search-box">
      <SearchField
        testid="{common.testid}-search"
        placeholder={searchPlaceholder}
        label={searchPlaceholder}
        bind:value={() => str(search.value), (v) => search.set(v)}
      />
    </div>
    {#if scopes.length}
      <div class="scopes">
        <SegmentedControl
          testid="{common.testid}-scopes"
          label="Search scope"
          block
          segments={scopes.map((s) => ({ value: s, label: s }))}
          value={selectedScope}
          onchange={(v) => scopeBinding?.set(v)}
        />
      </div>
    {/if}
  {/if}
  {#if principal.length}<Children
      nodes={principal}
      {path}
      slotName="toolbar.principal"
      {scope}
    />{/if}
{/snippet}
{#snippet content()}
  {#if landmark === 'navigation'}
    <nav class="screen" aria-label={title || undefined} style={common.style}>
      <Children nodes={node.children} {path} {scope} />
    </nav>
  {:else if landmark === 'region'}
    <section class="screen" aria-label={title || 'Content'} style={common.style}>
      <Children nodes={node.children} {path} {scope} />
    </section>
  {:else}
    <main class="screen" style={common.style}>
      <Children nodes={node.children} {path} {scope} />
    </main>
  {/if}
{/snippet}
{#snippet bottomBar()}
  <Children nodes={bottom} {path} slotName="toolbar.bottom" {scope} />
{/snippet}

<ScreenTemplate
  testid={common.testid}
  {background}
  navigationBar={bar}
  {content}
  bottomBar={bottom.length ? bottomBar : undefined}
  onrefresh={refreshable ? () => view.fire(node, path, 'refresh', scope) : undefined}
/>

<style>
  .screen {
    display: block;
    min-width: 0;
  }
  .subtitle {
    margin: 0 0 8px;
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
  .scopes {
    margin-top: 8px;
  }
</style>
