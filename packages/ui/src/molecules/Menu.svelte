<!--
  @molecule Menu — a menu panel: items, separators, section headings, checkmark items (toggles and the chosen
  option of a group), destructive items and nested submenus. Two looks: `mac` (the macOS pull-down, used by the
  menu bar) and `ios` (the iPadOS context menu: 44 pt rows, a thick material, trailing glyphs).
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
    variant?: 'mac' | 'ios'
    /** Move focus to the first item when the menu appears. */
    autofocus?: boolean
    /** Called after an item runs, so the owner can close the menu. */
    onselect?: () => void
    /** Escape on the root panel, or Tab: the owner closes the menu and returns focus. */
    onclose?: () => void
  }

  let { testid, items, label, variant = 'mac', autofocus = false, onselect, onclose }: Props = $props()

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

  function enter(item: MenuEntry, path: string): void {
    if (variant === 'mac' && item.submenu && !item.disabled) openPath = path
    else if (!item.submenu) {
      // Hovering a plain item closes any submenu opened from a sibling.
      const depth = path.split('.').length - 1
      if (openPath && openPath.split('.').length > depth) openPath = openPath.split('.').slice(0, depth).join('.') || null
    }
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
    class="items {variant}"
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
              {#if item.checked}<Icon name="checkmark" size={variant === 'ios' ? 16 : 11} weight={3} />{/if}
            </span>
            <span class="label">{item.label}</span>
            {#if item.shortcut}<span class="shortcut">{item.shortcut}</span>{/if}
            {#if item.icon}<span class="glyph" aria-hidden="true"><Icon name={item.icon} size={variant === 'ios' ? 19 : 14} weight={2} /></span>{/if}
            {#if item.submenu}<span class="chevron" aria-hidden="true"><Icon name="chevron.right" size={variant === 'ios' ? 14 : 10} weight={3} /></span>{/if}
          </button>
          {#if item.submenu && (openPath === path || openPath?.startsWith(`${path}.`))}
            {#if variant === 'ios'}
              <!-- iOS: a submenu opens in place under its item (a flyout would leave a narrow window). -->
              <div class="inline">{@render panel(item.submenu, depth + 1, path)}</div>
            {:else}
              <div class="sub">
                <Material thickness="thick" radius="8px">
                  {@render panel(item.submenu, depth + 1, path)}
                </Material>
              </div>
            {/if}
          {/if}
        </div>
      {/if}
    {/each}
  </div>
{/snippet}

<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
  class="ios-menu {variant}"
  data-testid={testid}
  bind:this={root}
  {onkeydown}
>
  <Material thickness="thick" radius={variant === 'ios' ? 'var(--radius-lg)' : '8px'}>
    {@render panel(items, 0, '')}
  </Material>
</div>

<style>
  .ios-menu {
    position: relative;
    min-width: 220px;
    border-radius: 8px;
    color: var(--label);
  }
  .ios-menu.mac {
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.18),
      0 10px 30px rgb(0 0 0 / 0.25);
  }
  .ios-menu.ios {
    min-width: 240px;
    max-width: min(320px, calc(100vw - 16px));
    border-radius: var(--radius-lg);
    box-shadow: var(--shadow-popover);
  }
  .items {
    position: relative;
    display: flex;
    flex-direction: column;
    outline: 0;
  }
  .items.mac {
    padding: 5px;
    border-radius: 8px;
    box-shadow: inset 0 0 0 0.5px rgb(255 255 255 / 0.2);
  }
  .items.ios {
    padding: 0;
    border-radius: var(--radius-lg);
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
    gap: 4px;
    width: 100%;
    color: inherit;
    text-align: start;
    cursor: default;
  }
  .mac .item {
    height: 22px;
    padding: 0 10px 0 4px;
    border-radius: 4px;
    font: 400 13px/16px var(--font-text);
  }
  .ios .item {
    min-height: var(--row-height);
    padding: 0 16px;
    gap: 10px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    cursor: pointer;
  }
  .ios .slot + .slot > .item,
  .ios .slot + .separator + .slot > .item {
    border-top: var(--hairline) solid var(--separator);
  }
  .mac .item:hover:not(:disabled),
  .mac .item:focus-visible,
  .mac .item[aria-expanded='true'] {
    outline: none;
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  .ios .item:hover:not(:disabled),
  .ios .item:active:not(:disabled),
  .ios .item[aria-expanded='true'] {
    background: var(--fill-secondary);
  }
  .ios .item:focus-visible {
    outline: 3px solid color-mix(in srgb, var(--tint) 55%, transparent);
    outline-offset: -3px;
  }
  .item:disabled {
    color: var(--label-tertiary);
  }
  .mac .destructive {
    color: var(--ios-red);
  }
  .ios .destructive {
    color: var(--ios-red);
  }
  .check {
    display: flex;
    justify-content: center;
    flex: none;
  }
  .mac .check {
    width: 14px;
  }
  /* iOS: the checkmark column exists only in menus that have checkmark items. */
  .ios .check {
    width: 0;
  }
  .ios .marks .check {
    width: 18px;
  }
  .label {
    flex: 1;
    min-width: 0;
  }
  .mac .label {
    white-space: nowrap;
  }
  .ios .label {
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
  .mac .item:hover:not(:disabled) .glyph,
  .mac .item:focus-visible .glyph,
  .mac .item[aria-expanded='true'] .glyph,
  .mac .item:hover:not(:disabled) .chevron,
  .mac .item:focus-visible .chevron,
  .mac .item[aria-expanded='true'] .chevron {
    color: #fff;
  }
  .shortcut {
    margin-left: 24px;
    color: var(--label-secondary);
    letter-spacing: 1px;
  }
  .mac .item:hover:not(:disabled) .shortcut,
  .mac .item:focus-visible .shortcut {
    color: rgb(255 255 255 / 0.85);
  }
  .separator {
    background: var(--separator);
  }
  .mac .separator {
    height: 1px;
    margin: 5px 10px;
  }
  .ios .separator {
    height: 8px;
    background: var(--fill-quaternary);
  }
  .heading {
    font: var(--type-footnote);
    color: var(--label-secondary);
    text-transform: uppercase;
    letter-spacing: var(--tracking-footnote);
  }
  .mac .heading {
    padding: 6px 10px 2px;
  }
  .ios .heading {
    padding: 10px 16px 4px;
  }
  .sub {
    position: absolute;
    z-index: 1;
    top: -5px;
    inset-inline-start: 100%;
    min-width: 200px;
    border-radius: 8px;
    box-shadow: var(--shadow-popover);
  }
  .inline {
    background: var(--fill-quaternary);
    padding-inline-start: 12px;
  }
</style>
