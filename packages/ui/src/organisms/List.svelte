<!--
  @organism List — a scrolling collection of rows in one of five styles (plain, inset, insetGrouped, grouped,
  sidebar). It owns what is shared by every row: the list semantics (`group` of lists, or a `listbox` when rows are
  selectable), ONE tab stop with arrow-key navigation (roving tabindex; Home, End, Page Up and Down; type to
  select), reordering (drag a row's handle, or `Alt+Arrow` on a row) and the polite live region that announces a
  move. Rows (`ListRow` with a `role`) and `ListSection`s go in `children`; virtualization is `VirtualRows`.
  With `onrefresh`: pull to refresh and a keyboard "Refresh" button. Atoms: ActivityIndicator. Molecules: RefreshKey.
  Test ids: `testid` on the container, `${testid}-announcer` on the live region, `${testid}-refresh` on the button.

  Keys are strings here (the DOM's `data-row-key`); the caller maps them back to its row keys.
-->
<script lang="ts">
  import { onMount, setContext, tick, type Snippet } from 'svelte'
  import ActivityIndicator from '../atoms/ActivityIndicator.svelte'
  import RefreshKey from '../molecules/RefreshKey.svelte'
  import { PULL_THRESHOLD } from '../logic/gestures'
  import { pullToRefresh } from '../logic/pullToRefresh'
  import { LIST_CONTEXT, type ListStyle } from '../logic/listContext'
  import { listKeyAction, TypeaheadBuffer, typeaheadMatch } from '../logic/listKeys'
  import { insertionIndex } from '../logic/reorder'

  interface Props {
    testid: string
    style?: ListStyle
    /** `single` / `multi` make the list a `listbox` of selectable rows. */
    selection?: 'none' | 'single' | 'multi'
    label?: string
    /** Reordering is allowed (`Alt+Arrow`, and dragging a row's handle). */
    reorderable?: boolean
    /** `Alt+Arrow` on row `key`: move one step. */
    onreorder?: (key: string, delta: -1 | 1) => void
    /** A drag ended: `key` goes in front of `before` (null = after the last rendered row). */
    onmove?: (key: string, before: string | null) => void
    /** Home or End on a list with rows outside the DOM: scroll there; return true when you did. */
    onedge?: (edge: 'start' | 'end') => boolean
    /** Pull to refresh (touch) and a keyboard "Refresh" button; the spinner runs until this resolves. */
    onrefresh?: () => void | Promise<void>
    /** Said once by the live region (a move, a refresh). */
    announcement?: string
    /** Inline layout style from the caller. */
    css?: string
    children: Snippet
  }

  let {
    testid,
    style = 'insetGrouped',
    selection = 'none',
    label,
    reorderable = false,
    onreorder,
    onmove,
    onedge,
    onrefresh,
    announcement = '',
    css,
    children
  }: Props = $props()

  const selectable = $derived(selection !== 'none')
  setContext(LIST_CONTEXT, {
    get style() {
      return style
    },
    get selectable() {
      return selectable
    }
  })

  let root: HTMLElement | undefined = $state()
  let pulled = $state(0)
  let refreshing = $state(false)
  async function refresh(): Promise<void> {
    if (refreshing || !onrefresh) return
    refreshing = true
    try {
      await onrefresh()
    } finally {
      refreshing = false
    }
  }
  const typeahead = new TypeaheadBuffer()

  const focusables = (): HTMLElement[] =>
    root ? Array.from(root.querySelectorAll<HTMLElement>('[data-row-focus]')) : []
  const keyOf = (el: Element | null): string | null =>
    el?.closest<HTMLElement>('[data-row-key]')?.dataset.rowKey ?? null

  // ── roving tabindex: exactly one row is in the tab order ──
  let current: string | null = null
  function normalize(): void {
    const els = focusables()
    if (els.length === 0) return
    const chosen =
      els.find((el) => current !== null && keyOf(el) === current) ??
      els.find((el) => el.closest('[aria-selected="true"]') || el.getAttribute('aria-selected') === 'true') ??
      els[0]
    for (const el of els) el.tabIndex = el === chosen ? 0 : -1
  }
  onMount(() => {
    normalize()
    if (!root) return undefined
    const mo = new MutationObserver(normalize)
    mo.observe(root, { childList: true, subtree: true })
    return () => mo.disconnect()
  })

  function focusin(e: FocusEvent): void {
    const el = (e.target as HTMLElement).closest<HTMLElement>('[data-row-focus]')
    if (!el) return
    current = keyOf(el)
    for (const other of focusables()) other.tabIndex = other === el ? 0 : -1
  }

  const isTextEntry = (el: HTMLElement): boolean =>
    el.isContentEditable || ['INPUT', 'TEXTAREA', 'SELECT'].includes(el.tagName)

  function refocus(key: string | null): void {
    if (key === null) return
    requestAnimationFrame(() =>
      root
        ?.querySelector<HTMLElement>(`[data-row-key="${CSS.escape(key)}"] [data-row-focus], [data-row-key="${CSS.escape(key)}"][data-row-focus]`)
        ?.focus()
    )
  }

  /**
   * Home / End of a list with rows outside the DOM: the owner scrolls there, then focus follows once the row at that
   * end exists (its `aria-posinset` says so), which can take a window from the host.
   */
  async function edge(to: 'start' | 'end'): Promise<void> {
    if (!onedge?.(to)) return
    await tick()
    for (let frame = 0; frame < 90; frame++) {
      await new Promise((r) => requestAnimationFrame(r))
      const els = focusables()
      const el = to === 'start' ? els[0] : els.at(-1)
      const item = el?.closest<HTMLElement>('[aria-posinset]')
      const there =
        item &&
        (to === 'start'
          ? item.getAttribute('aria-posinset') === '1'
          : item.getAttribute('aria-posinset') === item.getAttribute('aria-setsize'))
      if (el && there) return void el.focus()
    }
  }

  function keydown(e: KeyboardEvent): void {
    const target = e.target as HTMLElement
    if (isTextEntry(target)) return
    const handle = target.closest('[data-reorder-handle]')
    const rowKey = keyOf(target)
    if (handle && rowKey !== null && (e.key === 'ArrowUp' || e.key === 'ArrowDown') && !e.altKey) {
      e.preventDefault()
      onreorder?.(rowKey, e.key === 'ArrowUp' ? -1 : 1)
      refocus(rowKey)
      return
    }
    const el = target.closest<HTMLElement>('[data-row-focus]')
    // Swipe actions: Escape closes, Left and Right move between the buttons.
    const actions = target.closest<HTMLElement>('[data-actions]')
    if (actions && rowKey !== null) {
      const buttons = Array.from(actions.querySelectorAll<HTMLElement>('[data-swipe-action]'))
      const at = buttons.indexOf(target)
      if (e.key === 'Escape') {
        e.preventDefault()
        rowEl(rowKey)?.dispatchEvent(new CustomEvent('swipeopen', { detail: 'close' }))
      } else if (at >= 0 && (e.key === 'ArrowLeft' || e.key === 'ArrowRight')) {
        e.preventDefault()
        // The trailing group is laid out right to left (its first action is at the edge): mirror the arrows.
        const step = (e.key === 'ArrowRight' ? 1 : -1) * (actions.dataset.actions === 'trailing' ? -1 : 1)
        buttons[(at + step + buttons.length) % buttons.length].focus()
      }
      return
    }
    if (!el || rowKey === null) return
    const els = focusables()
    const row = rowEl(rowKey)
    const action = listKeyAction(e, {
      index: els.indexOf(el),
      count: els.length,
      page: Math.max(1, Math.floor((root?.clientHeight ?? 400) / 48)),
      canReorder: reorderable && !!onreorder,
      swipe: row
        ? {
            leading: !!row.querySelector('[data-actions="leading"]'),
            trailing: !!row.querySelector('[data-actions="trailing"]'),
            open: row.dataset.open !== undefined
          }
        : undefined,
      rtl: getComputedStyle(root!).direction === 'rtl'
    })
    if (!action) return
    switch (action.type) {
      case 'focus':
        e.preventDefault()
        if (onedge && (e.key === 'Home' || e.key === 'End')) void edge(e.key === 'Home' ? 'start' : 'end')
        else els[action.index]?.focus()
        return
      case 'reorder':
        e.preventDefault()
        onreorder?.(rowKey, action.delta)
        refocus(rowKey)
        return
      case 'swipe':
        e.preventDefault()
        row?.dispatchEvent(new CustomEvent('swipeopen', { detail: action.side }))
        return
      case 'typeahead': {
        const query = typeahead.push(action.char, e.timeStamp)
        const labels = els.map((x) => x.getAttribute('aria-label') ?? x.textContent ?? '')
        const hit = typeaheadMatch(labels, query, els.indexOf(el))
        if (hit >= 0) els[hit].focus()
      }
    }
  }

  const rowEl = (key: string): HTMLElement | null =>
    root?.querySelector<HTMLElement>(`[data-row-key="${CSS.escape(key)}"]`) ?? null

  // ── reorder by dragging a handle ──
  let drag: { key: string; el: HTMLElement; startY: number; pointer: number; scroller: HTMLElement | null; raf: number } | null = null

  function scrollParent(el: HTMLElement): HTMLElement | null {
    for (let n = el.parentElement; n; n = n.parentElement) {
      const o = getComputedStyle(n).overflowY
      if (o === 'auto' || o === 'scroll') return n
    }
    return null
  }

  function pointerdown(e: PointerEvent): void {
    const handle = (e.target as HTMLElement).closest<HTMLElement>('[data-reorder-handle]')
    const el = handle?.closest<HTMLElement>('[data-row-key]')
    if (!handle || !el || !el.dataset.rowKey || e.button !== 0) return
    handle.setPointerCapture(e.pointerId)
    drag = { key: el.dataset.rowKey, el, startY: e.clientY, pointer: e.clientY, scroller: scrollParent(el), raf: 0 }
    el.dataset.dragging = ''
    const scrollLoop = (): void => {
      if (!drag?.scroller) return
      const r = drag.scroller.getBoundingClientRect()
      const margin = 40
      const dy = drag.pointer < r.top + margin ? -10 : drag.pointer > r.bottom - margin ? 10 : 0
      if (dy) {
        drag.scroller.scrollTop += dy
        place()
      }
      drag.raf = requestAnimationFrame(scrollLoop)
    }
    drag.raf = requestAnimationFrame(scrollLoop)
  }

  function others(): HTMLElement[] {
    return root && drag
      ? Array.from(root.querySelectorAll<HTMLElement>('[data-row-key]')).filter((r) => r !== drag!.el)
      : []
  }

  function place(): void {
    if (!drag) return
    drag.el.style.transform = `translateY(${drag.pointer - drag.startY}px)`
    const rows = others()
    const at = insertionIndex(rows.map((r) => r.getBoundingClientRect()), drag.pointer)
    for (const r of rows) delete r.dataset.drop
    if (at < rows.length) rows[at].dataset.drop = 'before'
    else if (rows.length) rows[rows.length - 1].dataset.drop = 'after'
  }

  function pointermove(e: PointerEvent): void {
    if (!drag) return
    drag.pointer = e.clientY
    place()
  }

  function finish(commit: boolean): void {
    if (!drag) return
    const { key, el, raf } = drag
    const rows = others()
    const at = insertionIndex(rows.map((r) => r.getBoundingClientRect()), drag.pointer)
    cancelAnimationFrame(raf)
    for (const r of rows) delete r.dataset.drop
    delete el.dataset.dragging
    el.style.transform = ''
    drag = null
    if (commit) {
      onmove?.(key, at < rows.length ? (rows[at].dataset.rowKey ?? null) : null)
      refocus(key)
    }
  }
</script>

<div
  class="wrap"
  aria-busy={refreshing || undefined}
  use:pullToRefresh={{
    enabled: !!onrefresh,
    onrefresh: refresh,
    scroller: () => (root ? scrollParent(root) : null),
    onchange: (d, r) => {
      pulled = d
      refreshing = r
    }
  }}
>
  {#if onrefresh}
    <RefreshKey testid="{testid}-refresh" onclick={refresh} disabled={refreshing} />
    <div class="pull" style:height="{refreshing ? PULL_THRESHOLD : Math.min(pulled, PULL_THRESHOLD)}px" aria-hidden="true">
      {#if refreshing || pulled > 8}<ActivityIndicator label="Refreshing" />{/if}
    </div>
  {/if}
  <!-- svelte-ignore a11y_no_noninteractive_element_interactions (the list is the keyboard owner of its rows) -->
  <div
  class="ios-list {style}"
  class:sidebar={style === 'sidebar'}
  data-testid={testid}
  role={selectable ? 'listbox' : 'group'}
  aria-label={label}
  aria-multiselectable={selection === 'multi' ? true : undefined}
  bind:this={root}
  onfocusin={focusin}
  onkeydown={keydown}
  onpointerdown={pointerdown}
  onpointermove={pointermove}
  onpointerup={() => finish(true)}
  onpointercancel={() => finish(false)}
  style={css}
>
  {@render children()}
  </div>
</div>
<div class="sr-only" role="status" aria-live="polite" data-testid="{testid}-announcer">{announcement}</div>

<style>
  .wrap {
    min-width: 0;
  }
  .pull {
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    transition: height var(--duration-fast) var(--ease-ios);
  }
  @media (prefers-reduced-motion: reduce) {
    .pull {
      transition: none;
    }
  }
  .ios-list {
    --list-inset: 16px;
    min-width: 0;
    background: var(--bg-grouped-elevated);
  }
  .plain,
  .inset {
    background: var(--bg-elevated);
  }
  /* Dragging: the row follows the pointer; a line shows where it will land. */
  .ios-list :global([data-dragging]) {
    position: relative;
    z-index: 20;
    box-shadow: var(--shadow-popover);
    background: var(--bg-elevated);
    transition: none;
  }
  .ios-list :global([data-drop='before'])::before,
  .ios-list :global([data-drop='after'])::after {
    content: '';
    position: absolute;
    left: var(--row-inset);
    right: 0;
    height: 2px;
    border-radius: 1px;
    background: var(--tint);
    z-index: 15;
  }
  .ios-list :global([data-drop='before'])::before {
    top: -1px;
  }
  .ios-list :global([data-drop='after'])::after {
    bottom: -1px;
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
