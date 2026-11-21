<!--
  @molecule Menu — a menu panel: items, separators, section headings, checkmark items (toggles and the chosen
  option of a group), destructive items and nested submenus, in the iPadOS context-menu look (44 pt rows, a thick
  material, trailing glyphs; a submenu opens in place under its item).
  Keyboard: Up/Down/Home/End move (disabled items skipped), Enter or Space runs, Right opens a submenu, Left or
  Escape closes it (Escape on the root calls `onclose`), type to jump to a label, Tab closes. `autofocus` moves
  focus into the menu when it appears. Atoms: Material, Icon.
  Test ids: `testid` on the menu, `${testid}-panel` on its list, `${testid}-<item key or label slug>` on each item.
-->
<script lang="ts">
  import Material from '../atoms/Material.svelte'
  import Icon from '../atoms/Icon.svelte'
  import { isPrintable, moveIndex, nextEnabled, typeAhead } from '../keys'
  import type { MenuEntry, MenuItem } from '../types'

  interface Props {
    testid: string
    items: MenuItem[]
    label?: string
    /** Move focus to the first item when the menu appears. */
    autofocus?: boolean
    /** Called after an item runs, so the owner can close the menu. */
    onselect?: () => void
    /** Escape on the root panel, or Tab: the owner closes the menu and returns focus. */
    onclose?: () => void
  }

  let { testid, items, label, autofocus = false, onselect, onclose }: Props = $props()

  const slug = (text: string): string =>
    text.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  /** The submenu that is open, as a path of item positions ("2", "2.0"); null when none. */
  let openPath: string | null = $state(null)
  let root: HTMLElement | undefined = $state()
  let buffer = ''
  let bufferTimer: ReturnType<typeof setTimeout> | undefined

  const entries = (list: MenuItem[]): MenuEntry[] =>
    list.filter((i): i is MenuEntry => 'label' in i)
  const hasMarks = (list: MenuItem[]): boolean => entries(list).some((e) => e.checked !== undefined)

  function run(item: MenuEntry): void {
    if (item.disabled) return
    if (item.submenu) return
    item.onclick?.()
    onselect?.()
  }

  function panelOf(el: Element): HTMLElement | null {
    return el.closest<HTMLElement>('[data-menu-panel]')
  }
  function itemsOf(panel: HTMLElement): HTMLElement[] {
    return [...panel.querySelectorAll<HTMLElement>('[data-menu-item]')].filter(
      (el) => el.closest('[data-menu-panel]') === panel
    )
  }

  /** Focus the first item of a freshly opened panel (after it renders). */
  function focusFirst(panel: HTMLElement | null | undefined): void {
    if (!panel) return
    const list = itemsOf(panel).filter((b) => !(b as HTMLButtonElement).disabled)
    list[0]?.focus()
  }

  $effect(() => {
    if (!autofocus || !root) return undefined
    const frame = requestAnimationFrame(() => focusFirst(root?.querySelector<HTMLElement>('[data-menu-panel]')))
    return () => cancelAnimationFrame(frame)
  })

  /** Close the submenu opened from the item at `itemPath` and put focus back on that item. */
  function closeSubmenu(itemPath: string): void {
    const up = itemPath.split('.').slice(0, -1).join('.')
    openPath = up === '' ? null : up
    queueMicrotask(() => root?.querySelector<HTMLElement>(`[data-path="${itemPath}"]`)?.focus())
  }

  function onkeydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement
    const panel = panelOf(target)
    if (!panel) return
    const buttons = itemsOf(panel)
    const current = buttons.indexOf(target.closest<HTMLElement>('[data-menu-item]') as HTMLElement)
    const flags = buttons.map((b) => (b as HTMLButtonElement).disabled)

    const depth = Number(panel.dataset.depth)
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      if (depth > 0) closeSubmenu(panel.dataset.for ?? '')
      else onclose?.()
      return
    }
    if (e.key === 'Tab') {
      onclose?.()
      return
    }
    if (e.key === 'ArrowLeft' && depth > 0) {
      e.preventDefault()
      closeSubmenu(panel.dataset.for ?? '')
      return
    }
    if (e.key === 'ArrowRight' && target.dataset.submenu === 'true') {
      e.preventDefault()
      openPath = target.dataset.path ?? null
      const path = openPath
      requestAnimationFrame(() => focusFirst(root?.querySelector<HTMLElement>(`[data-menu-panel][data-for="${path}"]`)))
      return
    }
    const to = moveIndex(buttons.length, current, e.key, 'vertical')
    if (to !== null) {
      e.preventDefault()
      const next =
        e.key === 'Home'
          ? nextEnabled(flags, -1, 1)
          : e.key === 'End'
            ? nextEnabled(flags, 0, -1)
            : nextEnabled(flags, current, e.key === 'ArrowUp' ? -1 : 1)
      buttons[next]?.focus()
      return
    }
    if (isPrintable(e)) {
      buffer += e.key
      clearTimeout(bufferTimer)
      bufferTimer = setTimeout(() => (buffer = ''), 600)
      const labels = buttons.map((b) => b.dataset.label ?? '')
      const hit = typeAhead(labels, buffer, buffer.length === 1 ? current : current - 1)
      if (hit >= 0) buttons[hit]?.focus()
    }
  }

  /** Hovering a plain item closes any submenu opened from a sibling. */
  function enter(item: MenuEntry, path: string): void {
    if (item.submenu) return
    const depth = path.split('.').length - 1
    if (openPath && openPath.split('.').length > depth) openPath = openPath.split('.').slice(0, depth).join('.') || null
  }

  function press(item: MenuEntry, path: string): void {
    if (item.submenu) {
      openPath = openPath === path ? null : path
      if (openPath)
        requestAnimationFrame(() => focusFirst(root?.querySelector<HTMLElement>(`[data-menu-panel][data-for="${path}"]`)))
      return
    }
    run(item)
  }
</script>

{#snippet panel(list: MenuItem[], depth: number, parentPath: string)}
  {@const marks = hasMarks(list)}
  <div
    class="items"
    class:marks
    role="menu"
    data-menu-panel
    data-testid={depth === 0 ? `${testid}-panel` : `${testid}-submenu-${parentPath}`}
    data-depth={depth}
    data-for={parentPath}
    aria-label={depth === 0 ? label : undefined}
    tabindex="-1"
  >
    {#each list as item, i (i)}
      {@const path = parentPath === '' ? String(i) : `${parentPath}.${i}`}
      {#if 'separator' in item}
        <div class="separator" role="separator"></div>
      {:else if 'heading' in item}
        <div class="heading" role="presentation">{item.heading}</div>
      {:else}
        <div class="slot" class:has-sub={!!item.submenu}>
          <button
            type="button"
            role={item.submenu ? 'menuitem' : item.checked === undefined ? 'menuitem' : 'menuitemcheckbox'}
            aria-checked={item.submenu || item.checked === undefined ? undefined : item.checked}
            aria-haspopup={item.submenu ? 'menu' : undefined}
            aria-expanded={item.submenu ? openPath === path || (openPath?.startsWith(`${path}.`) ?? false) : undefined}
            data-testid="{testid}-{item.key ?? slug(item.label)}"
            data-menu-item
            data-path={path}
            data-label={item.label}
            data-submenu={item.submenu ? 'true' : undefined}
            class="item"
            class:destructive={item.destructive}
            disabled={item.disabled}
            tabindex="-1"
            onclick={() => press(item, path)}
            onpointerenter={() => enter(item, path)}
          >
            <span class="check" aria-hidden="true">
              {#if item.checked}<Icon name="checkmark" size={16} weight={3} />{/if}
            </span>
            <span class="label">{item.label}</span>
            {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
            {#if item.icon}<span class="glyph" aria-hidden="true"><Icon name={item.icon} size={19} weight={2} /></span>{/if}
            {#if item.submenu}<span class="chevron" aria-hidden="true"><Icon name="chevron.right" size={14} weight={3} /></span>{/if}
          </button>
          {#if item.submenu && (openPath === path || openPath?.startsWith(`${path}.`))}
            <!-- a submenu opens in place under its item (a flyout would leave a narrow window) -->
            <div class="inline">{@render panel(item.submenu, depth + 1, path)}</div>
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="ios-menu"
  data-testid={testid}
  bind:this={root}
  {onkeydown}
>
  <Material thickness="thick" radius="var(--radius-lg)">
    {@render panel(items, 0, '')}
  </Material>
</div>

<style>
  .ios-menu {
    position: relative;
    min-width: 240px;
    max-width: min(320px, calc(100vw - 16px));
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-popover);
    color: var(--label);
  }
  .items {
    position: relative;
    display: flex;
    flex-direction: column;
    padding: 0;
    border-radius: var(--radius-lg);
    outline: 0;
  }
  .slot {
    position: relative;
    display: flex;
    flex-direction: column;
  }
  .item {
    appearance: none;
    border: 0;
    background: transparent;
    display: flex;
    align-items: center;
    gap: 10px;
    width: 100%;
    min-height: var(--row-height);
    padding: 0 16px;
    color: inherit;
    text-align: start;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    cursor: pointer;
  }
  .slot + .slot > .item,
  .slot + .separator + .slot > .item {
    border-top: var(--hairline) solid var(--separator);
  }
  .item:hover:not(:disabled),
  .item:active:not(:disabled),
  .item[aria-expanded='true'] {
    background: var(--fill-secondary);
  }
  .item:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: -3px;
  }
  .item:disabled {
    color: var(--label-tertiary);
  }
  .destructive {
    color: var(--ios-red);
  }
  /* the checkmark column exists only in menus that have checkmark items */
  .check {
    display: flex;
    justify-content: center;
    flex: none;
    width: 0;
  }
  .marks .check {
    width: 18px;
  }
  .label {
    flex: 1;
    min-width: 0;
    overflow-wrap: anywhere;
  }
  .glyph,
  .chevron {
    display: grid;
    flex: none;
    color: var(--label-secondary);
  }
  .destructive .glyph {
    color: var(--ios-red);
  }
  .shortcut {
    margin-left: 24px;
    color: var(--label-secondary);
    letter-spacing: 1px;
  }
  .separator {
    height: 8px;
    background: var(--fill-quaternary);
  }
  .heading {
    padding: 10px 16px 4px;
    font: var(--type-footnote);
    color: var(--label-secondary);
    text-transform: uppercase;
    letter-spacing: var(--tracking-footnote);
  }
  .inline {
    background: var(--fill-quaternary);
    padding-inline-start: 12px;
  }
</style>
