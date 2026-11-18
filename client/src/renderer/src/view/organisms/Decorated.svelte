<!--
  @organism Decorated — the common props every node accepts that are behaviour rather than style, wrapped around
  an adapter (`display: contents`, no box of its own):
   • `contextMenu` slot: a menu of Buttons, Toggles, Dividers, Pickers, submenus. Opened by right-click, a long press
     (touch or pen), or the keyboard (Shift+F10 or the Menu key, at the focused element).
   • `drag` / `drop` props and the `drop` event: pointer-based dragging with a ghost, highlight of accepting targets,
     auto-scroll and Escape to cancel. Every draggable also has a non-pointer path: Alt+Arrow moves it to the next or
     previous accepting target, and its context menu gets a "Move to" submenu listing the targets.
  The `drop` event carries `{type, payload, target: {key, beforeKey, edge}}`. Molecules: Menu, Popover.
-->
<script lang="ts">
  import { Menu, Popover } from '@agentos/ui/molecules'
  import type { MenuItem } from '@agentos/ui'
  import { isContextMenuKey } from '@agentos/ui'
  import { useView } from '../context'
  import {
    keyboardEdge,
    parseDrag,
    parseDrop,
    pointerEdge,
    type DropEvent,
    type DropTarget
  } from '../core/dnd'
  import { menuItems } from '../core/menuItems'
  import { testid } from '../core/testid'
  import type { Json } from '../protocol'
  import type { DecoratorProps } from '../registry'
  import { dragControllerOf } from '../kit/dragController'
  import { portalToWindow, restoreFocusTo } from '../overlay'

  let { node, path, scope, children }: DecoratorProps = $props()
  const ctx = useView()
  const { view } = ctx
  const controller = dragControllerOf(view)

  const apple = typeof navigator !== 'undefined' && /Mac|iPhone|iPad/.test(navigator.platform)
  const MOVE_PX = 6
  const ARM_MS = 350
  const MENU_MS = 500

  const drag = $derived(parseDrag(view.evaluate(node.props?.drag, scope)))
  const drop = $derived(parseDrop(view.evaluate(node.props?.drop, scope)))
  const menuNodes = $derived(node.slots?.contextMenu ?? [])
  const id = $derived(`${path}|${scope.rowKey ?? ''}`)
  const baseId = $derived(testid(ctx.app, path, scope.rowKey, node.props?.testid))
  const label = $derived.by(() => {
    const l = view.evaluate(node.props?.label, scope) ?? view.evaluate(node.props?.title, scope)
    return typeof l === 'string' && l ? l : node.key
  })

  let wrapper: HTMLElement | undefined = $state()
  let over = $state(false)

  // ── Document order, geometry ─────────────────────────────────────────
  const root = (): HTMLElement | null =>
    wrapper?.closest<HTMLElement>(`[data-testid="${CSS.escape(`${ctx.app}-view`)}"]`) ?? null
  const orderOf = (el: Element | undefined): number =>
    el ? [...(root()?.querySelectorAll('[data-dnd]') ?? [])].indexOf(el) : -1
  const box = (): HTMLElement | null => (wrapper?.firstElementChild as HTMLElement | null) ?? null

  // ── Drop target registration ─────────────────────────────────────────
  function applyDrop(event: DropEvent): void {
    view.fire(node, path, 'drop', scope, event as unknown as Json)
  }

  $effect(() => {
    const spec = drop
    const el = wrapper
    if (!spec || !el) return undefined
    const target: DropTarget = {
      id,
      key: node.key,
      label,
      accepts: spec.accepts,
      order: () => orderOf(el),
      rect: () => {
        const r = box()?.getBoundingClientRect()
        return r ? { x: r.left, y: r.top, width: r.width, height: r.height } : null
      },
      holds: (other) => el.contains(other),
      // A target that is not itself draggable is a container (a column, even an empty one): drops land inside it.
      isContainer: () => drag === null,
      drop: applyDrop
    }
    return controller.register(target)
  })

  $effect(() => {
    const spec = drop
    if (!spec?.highlight) return undefined
    const sync = (): void => {
      over = controller.over === id
    }
    sync()
    return controller.onChange(sync)
  })

  // ── Moving by menu and keyboard ──────────────────────────────────────
  function moveTo(target: DropTarget, dir: 1 | -1): void {
    if (!drag) return
    const edge = keyboardEdge(target.isContainer(), dir)
    target.drop({
      type: drag.type,
      payload: drag.payload ?? null,
      target: { key: target.key, beforeKey: null, edge }
    })
    view.announce(`Moved to ${target.label}`)
    const focusId = baseId
    requestAnimationFrame(() => restoreFocusTo(ctx.app, focusId, wrapper ?? null))
  }

  function destinations(): DropTarget[] {
    if (!drag || !wrapper) return []
    return controller.candidates(drag.type, id).filter((t) => !t.holds(wrapper!))
  }

  function keyMove(e: KeyboardEvent): void {
    if (!drag || !wrapper || !e.altKey || e.ctrlKey || e.metaKey) return
    const dir =
      e.key === 'ArrowDown' || e.key === 'ArrowRight'
        ? 1
        : e.key === 'ArrowUp' || e.key === 'ArrowLeft'
          ? -1
          : 0
    if (dir === 0) return
    // Only the innermost draggable around the focus handles it.
    if ((e.target as Element).closest('[data-drag-id]') !== wrapper) return
    e.preventDefault()
    const target = controller.step(drag.type, id, orderOf(wrapper), dir, (t) => t.holds(wrapper!))
    if (target) moveTo(target, dir)
    else view.announce(dir > 0 ? 'No later place to move to' : 'No earlier place to move to')
  }

  // ── Context menu ─────────────────────────────────────────────────────
  let menuOpen = $state(false)
  let point = $state<{ x: number; y: number } | null>(null)
  let returnFocus: HTMLElement | null = null

  const items = $derived.by<MenuItem[]>(() => {
    if (!menuOpen) return []
    const own = menuItems({ view, scope, apple }, menuNodes, path, 'contextMenu')
    const targets = destinations()
    if (!drag || targets.length === 0) return own
    const move: MenuItem = {
      key: 'move-to',
      label: 'Move to',
      icon: 'folder',
      submenu: targets.map((t) => ({
        key: `move-${t.key}`,
        label: t.label,
        onclick: () => moveTo(t, 1)
      }))
    }
    return own.length ? [...own, { separator: true }, move] : [move]
  })

  function openMenu(at: { x: number; y: number }): void {
    returnFocus = document.activeElement instanceof HTMLElement ? document.activeElement : null
    point = at
    menuOpen = true
  }
  function closeMenu(): void {
    menuOpen = false
    const back = returnFocus
    returnFocus = null
    queueMicrotask(() => back?.focus({ preventScroll: true }))
  }
  const hasMenu = $derived(menuNodes.length > 0 || drag !== null)

  function oncontextmenu(e: MouseEvent): void {
    if (!hasMenu || (e.target as Element).closest('[data-drag-id], [data-ctx-id]') !== wrapper)
      return
    e.preventDefault()
    openMenu({ x: e.clientX, y: e.clientY })
  }

  function onkeydown(e: KeyboardEvent): void {
    if (
      hasMenu &&
      isContextMenuKey(e) &&
      (e.target as Element).closest('[data-drag-id], [data-ctx-id]') === wrapper
    ) {
      e.preventDefault()
      const r =
        (e.target as HTMLElement).getBoundingClientRect?.() ?? box()?.getBoundingClientRect()
      if (r) openMenu({ x: r.left + Math.min(r.width / 2, 40), y: r.bottom })
      return
    }
    keyMove(e)
  }

  // ── Pointer dragging ─────────────────────────────────────────────────
  let press: {
    x: number
    y: number
    pointerId: number
    touch: boolean
    armed: boolean
    dragging: boolean
    armTimer?: ReturnType<typeof setTimeout>
    menuTimer?: ReturnType<typeof setTimeout>
  } | null = null
  let ghost: HTMLElement | null = null
  let frame = 0
  let last = { x: 0, y: 0 }

  const interactive = (el: Element): boolean =>
    !!el.closest('input, textarea, select, [contenteditable="true"], [role="slider"]')

  function onpointerdown(e: PointerEvent): void {
    if (e.button !== 0 && e.pointerType === 'mouse') return
    const mine = (e.target as Element).closest('[data-drag-id], [data-ctx-id]') === wrapper
    if (!mine || interactive(e.target as Element)) return
    const touch = e.pointerType !== 'mouse'
    press = {
      x: e.clientX,
      y: e.clientY,
      pointerId: e.pointerId,
      touch,
      armed: !touch,
      dragging: false
    }
    if (touch) {
      if (drag) press.armTimer = setTimeout(() => press && (press.armed = true), ARM_MS)
      if (hasMenu) {
        const at = { x: e.clientX, y: e.clientY }
        press.menuTimer = setTimeout(() => {
          if (press && !press.dragging) {
            clearTimeout(press.armTimer)
            press = null
            openMenu(at)
          }
        }, MENU_MS)
      }
    }
  }

  function startDrag(e: PointerEvent): void {
    if (!press || !drag || !wrapper) return
    press.dragging = true
    clearTimeout(press.menuTimer)
    try {
      wrapper.setPointerCapture(press.pointerId)
    } catch {
      /* the pointer is already gone */
    }
    controller.start(id, drag.type)
    const source = box()
    if (source) {
      const clone = source.cloneNode(true) as HTMLElement
      clone.querySelectorAll('[data-testid]').forEach((n) => n.removeAttribute('data-testid'))
      clone.removeAttribute('data-testid')
      clone.setAttribute('aria-hidden', 'true')
      const r = source.getBoundingClientRect()
      Object.assign(clone.style, {
        position: 'fixed',
        left: `${r.left}px`,
        top: `${r.top}px`,
        width: `${r.width}px`,
        margin: '0',
        pointerEvents: 'none',
        opacity: '0.85',
        boxShadow: 'var(--shadow-popover)',
        zIndex: '2000',
        transform: 'scale(1.02)'
      })
      clone.dataset.dragGhost = ''
      ;(root() ?? document.body).appendChild(clone)
      ghost = clone
      ghostOffset = { x: e.clientX - r.left, y: e.clientY - r.top }
    }
    document.addEventListener('keydown', onEscape, true)
    scrollLoop()
  }
  let ghostOffset = { x: 0, y: 0 }

  function onEscape(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.preventDefault()
      e.stopPropagation()
      endDrag(false)
    }
  }

  function onpointermove(e: PointerEvent): void {
    if (!press) return
    last = { x: e.clientX, y: e.clientY }
    if (!press.dragging) {
      if (Math.hypot(e.clientX - press.x, e.clientY - press.y) < MOVE_PX) return
      if (!drag || !press.armed) {
        // Moved before a touch was armed (the person is scrolling), or nothing to drag: not a drag.
        clearTimeout(press.armTimer)
        clearTimeout(press.menuTimer)
        press = null
        return
      }
      startDrag(e)
    }
    if (ghost) {
      ghost.style.left = `${e.clientX - ghostOffset.x}px`
      ghost.style.top = `${e.clientY - ghostOffset.y}px`
    }
    const hit = drag ? controller.hit(drag.type, { x: e.clientX, y: e.clientY }, id) : null
    controller.setOver(hit?.id ?? null)
    if (press?.touch && e.cancelable) e.preventDefault()
  }

  function onpointerup(e: PointerEvent): void {
    if (!press) return
    clearTimeout(press.armTimer)
    clearTimeout(press.menuTimer)
    if (press.dragging) {
      last = { x: e.clientX, y: e.clientY }
      endDrag(true)
    } else press = null
  }

  function endDrag(commit: boolean): void {
    cancelAnimationFrame(frame)
    document.removeEventListener('keydown', onEscape, true)
    ghost?.remove()
    ghost = null
    const spec = drag
    const target = spec && commit ? controller.hit(spec.type, last, id) : null
    controller.end()
    press = null
    if (spec && target) {
      const rect = target.rect()
      const edge = rect ? pointerEdge(rect, last, target.isContainer()) : 'inside'
      target.drop({
        type: spec.type,
        payload: spec.payload ?? null,
        target: { key: target.key, beforeKey: null, edge }
      })
      view.announce(`Dropped on ${target.label}`)
    } else if (spec && !commit) view.announce('Move cancelled')
  }

  /** While dragging near the edge of a scrollable ancestor of whatever is under the pointer, scroll it. */
  function scrollLoop(): void {
    const EDGE = 40
    const tick = (): void => {
      if (!press?.dragging) return
      for (const el of document.elementsFromPoint(last.x, last.y)) {
        const s = getComputedStyle(el)
        const scrolls = /(auto|scroll)/.test(s.overflowY) && el.scrollHeight > el.clientHeight
        if (!scrolls) continue
        const r = el.getBoundingClientRect()
        if (last.y < r.top + EDGE) el.scrollTop -= 12
        else if (last.y > r.bottom - EDGE) el.scrollTop += 12
        break
      }
      frame = requestAnimationFrame(tick)
    }
    frame = requestAnimationFrame(tick)
  }

  /** Once a touch drag is armed the page must not scroll under the finger. */
  function ontouchmove(e: TouchEvent): void {
    if (press?.armed && press.touch && drag && e.cancelable) e.preventDefault()
  }
  $effect(() => {
    const el = wrapper
    if (!el) return undefined
    el.addEventListener('touchmove', ontouchmove, { passive: false })
    return () => el.removeEventListener('touchmove', ontouchmove)
  })
  $effect(() => () => {
    ghost?.remove()
    cancelAnimationFrame(frame)
    document.removeEventListener('keydown', onEscape, true)
  })
</script>

<!-- svelte-ignore a11y_no_static_element_interactions (the wrapper only listens for the node's own context menu, drag and key paths) -->
<div
  class="decorated"
  data-testid="{baseId}-decorated"
  bind:this={wrapper}
  data-dnd={drag || drop ? '' : undefined}
  data-drag-id={drag ? id : undefined}
  data-ctx-id={hasMenu ? id : undefined}
  data-drop-id={drop ? id : undefined}
  data-over={over ? 'true' : undefined}
  {oncontextmenu}
  {onkeydown}
  {onpointerdown}
  {onpointermove}
  {onpointerup}
  onpointercancel={() => endDrag(false)}
>
  {@render children()}
</div>

{#if hasMenu}
  <div class="portal" {@attach portalToWindow(ctx.app)}>
    <Popover
      testid="{baseId}-context-popover"
      kind="menu"
      open={menuOpen}
      {point}
      arrow={false}
      onclose={closeMenu}
    >
      <Menu
        testid="{baseId}-context-menu"
        variant="ios"
        label="{label} actions"
        {items}
        autofocus
        onselect={closeMenu}
        onclose={closeMenu}
      />
    </Popover>
  </div>
{/if}

<style>
  .decorated,
  .portal {
    display: contents;
  }
  .decorated[data-drag-id] > :global(:first-child) {
    cursor: grab;
    touch-action: manipulation;
  }
  .decorated[data-over='true'] > :global(:first-child) {
    outline: 2px dashed var(--tint);
    outline-offset: -2px;
    border-radius: var(--radius-sm);
  }
</style>
