<!--
  @organism TabView — tabs with their panels, in one of four styles: `bottom` (UITabBar), `top` (an underlined
  tab strip), `sidebar` (a column of tabs beside the panel) and `page` (swipeable pages with dots). A real tab
  list: `role=tablist` / `tab` / `tabpanel`, one tab stop, Left / Right / Home / End move and select. Panels that
  were shown stay mounted (their scroll and state survive); the others are hidden. Atoms: Icon, Badge.
  Test ids: `testid` on the tab list, `${testid}-${id}` on each tab, `${testid}-panel-${id}` on each panel.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Badge from '../atoms/Badge.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'
  import { tabKeyIndex } from '../logic/listKeys'

  interface Tab {
    id: string
    label: string
    icon?: IconName
    badge?: number | string
  }

  interface Props {
    testid: string
    tabs: readonly Tab[]
    selected?: string
    style?: 'bottom' | 'top' | 'sidebar' | 'page'
    label?: string
    onselect?: (id: string) => void
    panel: Snippet<[string]>
    css?: string
  }

  let { testid, tabs, selected, style = 'bottom', label = 'Tabs', onselect, panel, css }: Props = $props()

  const uid = $props.id()
  const current = $derived(tabs.some((t) => t.id === selected) ? selected! : tabs[0]?.id)
  /** Panels shown at least once stay mounted. */
  let visited = $state<string[]>([])
  $effect(() => {
    if (current && !visited.includes(current)) visited = [...visited, current]
  })

  const horizontal = $derived(style !== 'sidebar')
  let tabEls: Record<string, HTMLElement | undefined> = $state({})

  function select(id: string, focus = false): void {
    if (id !== current) onselect?.(id)
    if (focus) queueMicrotask(() => tabEls[id]?.focus())
  }

  function keydown(e: KeyboardEvent): void {
    const index = tabs.findIndex((t) => t.id === current)
    const vertical = style === 'sidebar'
    const key = vertical ? ({ ArrowDown: 'ArrowRight', ArrowUp: 'ArrowLeft' }[e.key] ?? e.key) : e.key
    const next = tabKeyIndex(key, index, tabs.length, getComputedStyle(e.currentTarget as Element).direction === 'rtl' && !vertical)
    if (next === null) return
    e.preventDefault()
    select(tabs[next].id, true)
  }

  // `page`: swiping the pages selects the one that is most in view.
  let pages: HTMLElement | undefined = $state()
  $effect(() => {
    if (style !== 'page' || !pages) return undefined
    const io = new IntersectionObserver(
      (entries) => {
        const best = entries.filter((e) => e.isIntersecting).sort((a, b) => b.intersectionRatio - a.intersectionRatio)[0]
        const id = (best?.target as HTMLElement | undefined)?.dataset.panel
        if (id && best.intersectionRatio > 0.6) select(id)
      },
      { root: pages, threshold: [0.6, 1] }
    )
    for (const el of pages.querySelectorAll('[data-panel]')) io.observe(el)
    return () => io.disconnect()
  })
  $effect(() => {
    if (style !== 'page' || !pages || !current) return
    pages.querySelector<HTMLElement>(`[data-panel="${CSS.escape(current)}"]`)?.scrollIntoView({ inline: 'start', block: 'nearest' })
  })
</script>

{#snippet tabButtons()}
  <div
    class="tabs"
    role="tablist"
    aria-label={label}
    aria-orientation={horizontal ? 'horizontal' : 'vertical'}
    data-testid={testid}
    tabindex="-1"
    onkeydown={keydown}
  >
    {#each tabs as tab (tab.id)}
      {@const active = tab.id === current}
      <button
        type="button"
        class="tab"
        class:active
        role="tab"
        id="{uid}-tab-{tab.id}"
        aria-selected={active}
        aria-controls="{uid}-panel-{tab.id}"
        tabindex={active ? 0 : -1}
        data-testid="{testid}-{tab.id}"
        bind:this={tabEls[tab.id]}
        onclick={() => select(tab.id)}
      >
        {#if tab.icon && style !== 'page'}
          <span class="glyph"><Icon name={tab.icon} size={style === 'bottom' ? 24 : 20} weight={active ? 2.3 : 1.9} /></span>
        {/if}
        {#if style === 'page'}
          <span class="dot" aria-hidden="true"></span><span class="sr-only">{tab.label}</span>
        {:else}
          <span class="label">{tab.label}</span>
        {/if}
        {#if tab.badge !== undefined && tab.badge !== 0 && tab.badge !== ''}
          <span class="badge"><Badge value={tab.badge} /></span>
        {/if}
      </button>
    {/each}
  </div>
{/snippet}

{#snippet panels()}
  {#each tabs as tab (tab.id)}
    {#if visited.includes(tab.id) || tab.id === current}
      <div
        class="panel"
        role="tabpanel"
        id="{uid}-panel-{tab.id}"
        aria-labelledby="{uid}-tab-{tab.id}"
        data-panel={tab.id}
        data-testid="{testid}-panel-{tab.id}"
        hidden={style !== 'page' && tab.id !== current}
        inert={tab.id !== current}
      >
        {@render panel(tab.id)}
      </div>
    {/if}
  {/each}
{/snippet}

<div class="ios-tabview {style}" style={css}>
  {#if style === 'top' || style === 'sidebar'}{@render tabButtons()}{/if}
  {#if style === 'page'}
    <div class="pages" bind:this={pages}>
      {#each tabs as tab (tab.id)}
        <div
          class="panel slide"
          role="tabpanel"
          id="{uid}-panel-{tab.id}"
          aria-labelledby="{uid}-tab-{tab.id}"
          data-panel={tab.id}
          data-testid="{testid}-panel-{tab.id}"
        >
          {@render panel(tab.id)}
        </div>
      {/each}
    </div>
  {:else}
    <div class="body">{@render panels()}</div>
  {/if}
  {#if style === 'bottom' || style === 'page'}{@render tabButtons()}{/if}
</div>

<style>
  .ios-tabview {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-width: 0;
    min-height: 0;
    background: var(--bg-grouped-elevated);
  }
  .sidebar {
    flex-direction: row;
  }
  .body,
  .pages {
    flex: 1;
    min-width: 0;
    min-height: 0;
    position: relative;
  }
  .body {
    display: flex;
    flex-direction: column;
  }
  .pages {
    display: flex;
    overflow-x: auto;
    scroll-snap-type: x mandatory;
    scrollbar-width: none;
  }
  .pages::-webkit-scrollbar {
    display: none;
  }
  .panel {
    flex: 1;
    min-height: 0;
    min-width: 0;
    overflow: auto;
  }
  .panel[hidden] {
    display: none;
  }
  .slide {
    flex: 0 0 100%;
    scroll-snap-align: start;
  }

  .tabs {
    display: flex;
    gap: 4px;
    flex: none;
    background: var(--material-chrome);
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
  }
  .tab {
    appearance: none;
    border: 0;
    background: none;
    position: relative;
    color: var(--label-secondary);
    cursor: pointer;
    font: inherit;
  }
  .tab.active {
    color: var(--tint);
  }
  .tab:focus-visible {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
    border-radius: var(--radius-sm);
  }
  .glyph {
    position: relative;
    display: flex;
  }
  .badge {
    position: absolute;
  }

  /* bottom */
  .bottom .tabs,
  .page .tabs {
    justify-content: center;
    padding: 6px 8px 8px;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .bottom .tab {
    flex: 1;
    max-width: 120px;
    min-height: 44px;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 2px;
    padding: 2px 0;
  }
  .bottom .label {
    font: 500 10px/12px var(--font-text);
    letter-spacing: 0.1px;
  }
  .bottom .badge {
    top: 0;
    right: calc(50% - 26px);
  }

  /* top */
  .top .tabs {
    padding: 0 8px;
    overflow-x: auto;
    scrollbar-width: none;
    box-shadow: 0 var(--hairline) 0 var(--separator);
  }
  .top .tab {
    display: flex;
    align-items: center;
    gap: 6px;
    min-height: 44px;
    padding: 0 12px;
    white-space: nowrap;
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .top .tab.active::after {
    content: '';
    position: absolute;
    left: 12px;
    right: 12px;
    bottom: 0;
    height: 2px;
    border-radius: 1px;
    background: var(--tint);
  }

  /* sidebar */
  .sidebar .tabs {
    flex-direction: column;
    gap: 2px;
    width: min(240px, 40%);
    padding: 10px;
    overflow-y: auto;
    box-shadow: var(--hairline) 0 0 var(--separator);
    background: var(--bg-grouped-elevated);
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
  }
  .sidebar .tab {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
    padding: 0 10px;
    border-radius: var(--radius-md);
    text-align: left;
    color: var(--label);
    font: var(--type-body);
  }
  .sidebar .tab .glyph {
    color: var(--tint);
  }
  .sidebar .tab.active {
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  .sidebar .tab.active .glyph {
    color: #fff;
  }
  .sidebar .badge {
    position: static;
    margin-left: auto;
  }

  /* page */
  .page .tabs {
    background: transparent;
    -webkit-backdrop-filter: none;
    backdrop-filter: none;
    box-shadow: none;
    padding: 4px 0 8px;
    gap: 0;
  }
  .page .tab {
    width: 28px;
    height: 28px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .dot {
    width: 8px;
    height: 8px;
    border-radius: 50%;
    background: var(--label-tertiary);
  }
  .page .tab.active .dot {
    background: var(--label);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
