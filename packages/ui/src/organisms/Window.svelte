<!--
  @organism Window — iPadOS Stage Manager window. Controlled: the frame comes
  in as props and gestures report back through `onframechange`, so a window
  manager can lay windows out (floating, full screen, split, slide over) and
  animate between layouts. The "···" handle opens the multitasking menu.
  Atoms: MultitaskingDots. Molecules: MultitaskingMenu.

  Test ids: `testid` on the window, `${testid}-content`, `${testid}-handle`,
  `${testid}-multitasking` (the "···" dots), `${testid}-menu` with items
  `${testid}-menu-<fullscreen|split-left|split-right|slideover|floating|minimize|close>`,
  `${testid}-resize` (floating windows only).

  Resize: a floating window shows a corner grip (always visible, `--window-grip`: 3:1 on any body) that a pointer drags
  and that is also a button: Tab to it, then the arrow keys resize by 16 px (Shift: 64 px), to the minimum size and
  the screen's edge (the owner clamps the frame). When a resize reaches a limit the grip nudges back once (no nudge
  under `prefers-reduced-motion`) and takes the tint, a small label beside the grip says which ("Smallest size",
  "Narrowest", "Shortest", "Widest", "Tallest", "Largest size") for a couple of seconds, and the window says it in full
  in `${testid}-resize-status` (a polite live region: "<title> is at its smallest size", "can't get any narrower", ...;
  said when the limit is first reached, not on every key press after it). At its smallest size (both axes at the
  minimum) the grip stays tinted, whenever you look.
  The grip has the tooltip "Resize — drag or use arrow keys".

  The grip is last in the tab order on purpose: it is window chrome in the bottom-right corner, so Tab reaches it after
  the window's content, in reading order, and a person who never resizes is not made to pass it before the app's own
  controls. (Putting it first would cost every keyboard user a stop on every window.)

  Focus: the window itself is focusable (`tabindex="-1"`, so a click on its chrome or a drag of its handle lands
  there). When it is, focus moves on to the element in the content marked `data-window-focus`, if there is one, so a
  window whose keys are handled inside (an app's window-scoped shortcuts) keeps listening after a move or resize.
  `data-window-focus` is the convention for "this element takes the window's keys": one per window, `tabindex="-1"`.
  See client/README.md, "Focus hooks".
-->
<script lang="ts">
  import { motion } from '../motion'
  import { onDestroy, tick, type Snippet } from 'svelte'
  import { scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import MultitaskingDots from '../atoms/MultitaskingDots.svelte'
  import MultitaskingMenu from '../molecules/MultitaskingMenu.svelte'
  import type { Frame, WindowLayout } from '../types'

  interface Props {
    testid: string
    title: string
    x: number
    y: number
    width: number
    height: number
    minWidth?: number
    minHeight?: number
    z?: number
    focused?: boolean
    layout?: WindowLayout
    onframechange?: (frame: Frame) => void
    onfocus?: () => void
    onlayout?: (layout: WindowLayout) => void
    onminimize?: () => void
    onclose?: () => void
    children: Snippet
  }

  let {
    testid,
    title,
    x,
    y,
    width,
    height,
    minWidth = 320,
    minHeight = 240,
    z = 1,
    focused = true,
    layout = 'floating',
    onframechange,
    onfocus,
    onlayout,
    onminimize,
    onclose,
    children
  }: Props = $props()

  /** Only floating windows can be moved or resized directly. */
  const movable = $derived(layout === 'floating')

  type Gesture = { kind: 'move' | 'resize'; px: number; py: number; frame: Frame }
  let gesture: Gesture | null = $state(null)
  let menuOpen = $state(false)
  let menuEl: HTMLElement | undefined = $state()
  let contentEl: HTMLElement | undefined = $state()

  function forwardFocus(): void {
    contentEl?.querySelector<HTMLElement>('[data-window-focus]')?.focus({ preventScroll: true })
  }

  function begin(kind: Gesture['kind'], e: PointerEvent): void {
    onfocus?.()
    if (e.button !== 0 || !movable) return
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    gesture = { kind, px: e.clientX, py: e.clientY, frame: { x, y, width, height } }
  }

  function move(e: PointerEvent): void {
    if (!gesture) return
    const dx = e.clientX - gesture.px
    const dy = e.clientY - gesture.py
    const f = gesture.frame
    if (gesture.kind === 'move') onframechange?.({ ...f, x: f.x + dx, y: Math.max(0, f.y + dy) })
    else void resizeTo(f, f.width + dx, f.height + dy, false)
  }

  function end(): void {
    gesture = null
  }

  const ARROWS: Record<string, readonly [number, number]> = {
    ArrowRight: [1, 0],
    ArrowLeft: [-1, 0],
    ArrowDown: [0, 1],
    ArrowUp: [0, -1]
  }

  /** The keyboard's way to resize: the arrow keys on the focused grip. */
  function resizeByKey(e: KeyboardEvent): void {
    const arrow = ARROWS[e.key]
    if (!arrow || e.metaKey || e.ctrlKey || e.altKey) return
    e.preventDefault()
    onfocus?.()
    const step = e.shiftKey ? 64 : 16
    void resizeTo({ x, y, width, height }, width + arrow[0] * step, height + arrow[1] * step, true)
  }

  // ── The limit cue ──────────────────────────────────────────────────────

  type Limit = 'narrowest' | 'shortest' | 'smallest' | 'widest' | 'tallest' | 'largest'
  const SAID: Record<Limit, string> = {
    narrowest: "can't get any narrower",
    shortest: "can't get any shorter",
    smallest: 'is at its smallest size',
    widest: "can't get any wider here",
    tallest: "can't get any taller here",
    largest: 'is as large as the screen allows'
  }
  /** How long the nudge and the announcement last after the last time a resize hit a limit. */
  const CUE_MS = 2200
  let limit: Limit | null = $state(null)
  let nudge = $state(0) // each key press at a limit restarts the nudge (the grip alternates between two animations)
  let cueTimer: ReturnType<typeof setTimeout> | undefined
  onDestroy(() => clearTimeout(cueTimer))
  /** The same news, short enough for a small label beside the grip (the live region has the full sentence). */
  const SHORT: Record<Limit, string> = {
    narrowest: 'Narrowest',
    shortest: 'Shortest',
    smallest: 'Smallest size',
    widest: 'Widest',
    tallest: 'Tallest',
    largest: 'Largest size'
  }
  const status = $derived(limit ? `${title} ${SAID[limit]}` : '')
  /** Which way the grip pushes back: out when the person pulled in, in when they pulled out. */
  const push = $derived(limit === null ? null : ['widest', 'tallest', 'largest'].includes(limit) ? 'in' : 'out')
  /** At its smallest size the grip stays tinted: the window can't shrink more, whenever you look. */
  const atSmallest = $derived(width <= minWidth && height <= minHeight)

  /**
   * Ask for a size. The window never asks for less than its minimum; the owner may still give less than it was
   * asked (the screen's edge), which shows on the next tick. Either way the person is told: the grip tints and nudges
   * back, and the live region says it once.
   */
  async function resizeTo(from: Frame, wantW: number, wantH: number, byKey: boolean): Promise<void> {
    const next = { ...from, width: Math.max(minWidth, wantW), height: Math.max(minHeight, wantH) }
    onframechange?.(next)
    const narrowest = wantW < from.width && wantW <= minWidth
    const shortest = wantH < from.height && wantH <= minHeight
    if (narrowest || shortest) {
      const both = next.width <= minWidth && next.height <= minHeight
      return cue(both ? 'smallest' : narrowest ? 'narrowest' : 'shortest', byKey)
    }
    if (!onframechange) return
    await tick()
    const wider = width < next.width
    const taller = height < next.height
    cue(wider && taller ? 'largest' : wider ? 'widest' : taller ? 'tallest' : null, byKey)
  }

  function cue(kind: Limit | null, repeat: boolean): void {
    clearTimeout(cueTimer)
    if (kind !== null) {
      if (limit !== kind || repeat) nudge++
      cueTimer = setTimeout(() => cue(null, false), CUE_MS)
    }
    limit = kind // null: moving away from a limit, or letting go of it, ends the cue
  }

  function choose(next: WindowLayout): void {
    menuOpen = false
    onlayout?.(next)
  }

  function onwindowpointerdown(e: PointerEvent): void {
    if (!menuOpen) return
    const target = e.target as HTMLElement
    if (menuEl?.contains(target) || target.closest('.ios-mt-dots')) return
    menuOpen = false
  }
</script>

<svelte:window
  onpointerdown={onwindowpointerdown}
  onkeydown={(e) => e.key === 'Escape' && (menuOpen = false)}
/>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div
  class="ios-window {layout}"
  class:focused
  class:animating={gesture === null}
  role="dialog"
  tabindex="-1"
  data-testid={testid}
  aria-label={title}
  style:left="{x}px"
  style:top="{y}px"
  style:width="{width}px"
  style:height="{height}px"
  style:z-index={z}
  onpointerdown={() => onfocus?.()}
  onfocus={forwardFocus}
  transition:scale={{ start: 0.9, duration: motion(300), easing: cubicOut }}
>
  <div class="content" bind:this={contentEl} data-testid="{testid}-content">{@render children()}</div>

  <div
    class="handle"
    class:movable
    role="presentation"
    data-testid="{testid}-handle"
    onpointerdown={(e) => begin('move', e)}
    onpointermove={move}
    onpointerup={end}
    onpointercancel={end}
  >
    <span role="presentation" data-testid="{testid}-handle-dots" onpointerdown={(e) => e.stopPropagation()}>
      <MultitaskingDots
        testid="{testid}-multitasking"
        active={focused}
        label="{title} multitasking options"
        onclick={() => (menuOpen = !menuOpen)}
      />
    </span>
  </div>

  {#if menuOpen}
    <div class="menu" bind:this={menuEl}>
      <MultitaskingMenu
        testid="{testid}-menu"
        {layout}
        {title}
        onlayout={choose}
        onminimize={() => {
          menuOpen = false
          onminimize?.()
        }}
        onclose={() => {
          menuOpen = false
          onclose?.()
        }}
      />
    </div>
  {/if}

  {#if movable}
    <!-- Last in the tab order, on purpose (see the header). -->
    <button
      type="button"
      class="resize"
      class:limit={limit !== null || atSmallest}
      data-limit={limit}
      data-push={push}
      data-nudge={nudge % 2}
      aria-label="Resize {title}"
      aria-describedby="{testid}-resize-hint"
      aria-keyshortcuts="ArrowUp ArrowDown ArrowLeft ArrowRight"
      title="Resize — drag or use arrow keys"
      data-testid="{testid}-resize"
      onmousedown={(e) => e.preventDefault()}
      onpointerdown={(e) => {
        e.stopPropagation()
        begin('resize', e)
        forwardFocus() // a drag leaves the keys with the content; only Tab puts focus on the grip itself
      }}
      onpointermove={move}
      onpointerup={end}
      onpointercancel={end}
      onkeydown={resizeByKey}
    ></button>
    <span class="sr-only" id="{testid}-resize-hint">Drag, or use the arrow keys; Shift for larger steps</span>
    <span class="sr-only" role="status" aria-live="polite" data-testid="{testid}-resize-status">{status}</span>
    <!-- What the live region says, for the eye: a small flat label beside the grip while a limit is being met. -->
    <span class="limit-note" class:shown={limit !== null} aria-hidden="true" data-testid="{testid}-resize-note"
      >{limit ? SHORT[limit] : ''}</span
    >
  {/if}
</div>

<style>
  .ios-window {
    position: absolute;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    background: var(--bg-grouped-elevated);
    color: var(--label);
    box-shadow: var(--shadow-window-inactive);
  }
  .focused {
    box-shadow: var(--shadow-window);
  }
  /* Layout changes glide; live drags/resizes track the pointer exactly. */
  .animating {
    transition:
      left var(--duration-slow) var(--ease-ios),
      top var(--duration-slow) var(--ease-ios),
      width var(--duration-slow) var(--ease-ios),
      height var(--duration-slow) var(--ease-ios),
      box-shadow var(--duration) var(--ease-ios);
  }
  .slideover {
    box-shadow: var(--shadow-slideover);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    border-radius: inherit;
    display: flex;
    flex-direction: column;
    /* Navigation bars inside a window clear the multitasking handle. */
    --nav-top-inset: 10px;
  }
  .content > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .handle {
    position: absolute;
    top: 0;
    left: 30%;
    right: 30%;
    height: 22px;
    display: flex;
    justify-content: center;
    align-items: flex-start;
    z-index: 20;
  }
  .handle.movable {
    cursor: grab;
  }
  .handle :global(.ios-mt-dots) {
    padding-top: 6px;
    padding-bottom: 4px;
  }
  .menu {
    position: absolute;
    top: 26px;
    left: 50%;
    transform: translateX(-50%);
    z-index: 60;
    animation: pop var(--duration-fast) var(--ease-ios);
  }
  @keyframes pop {
    from {
      opacity: 0;
      transform: translateX(-50%) scale(0.92);
    }
  }
  /* iPadOS corner grip: a short, bold arc that follows the window's corner, always visible, a button for the keyboard.
     The button is the 44 pt touch target, mostly outside the window (22 px inside it), so it never covers a corner key's
     circle; the arc is 24 px, 5 px thick, and sits 4 px inside the window's corner. */
  .resize {
    position: absolute;
    right: -22px;
    bottom: -22px;
    width: 44px;
    height: 44px;
    padding: 0;
    border: 0;
    border-radius: var(--radius-xl);
    background: none;
    cursor: nwse-resize;
    touch-action: none;
    z-index: 20;
  }
  .resize::after {
    content: '';
    position: absolute;
    right: 26px;
    bottom: 26px;
    width: 24px;
    height: 24px;
    border-right: 5px solid var(--window-grip);
    border-bottom: 5px solid var(--window-grip);
    border-bottom-right-radius: calc(var(--radius-xl) - 4px);
    transition: border-color var(--duration) var(--ease-ios);
    pointer-events: none; /* the 44 px button is the target: the drawn arc reaches further into the corner than it */
  }
  /* A limit (the minimum size or the screen's edge): the arc takes the tint, and pushes back once. --tint holds 3:1
     on the light, dark and black bodies like the grip does. */
  .resize.limit::after {
    border-color: var(--tint);
  }
  .resize[data-push='out'][data-nudge='0']::after {
    animation: grip-push-out var(--duration-slow) var(--ease-ios);
  }
  .resize[data-push='out'][data-nudge='1']::after {
    animation: grip-push-out-again var(--duration-slow) var(--ease-ios);
  }
  .resize[data-push='in'][data-nudge='0']::after {
    animation: grip-push-in var(--duration-slow) var(--ease-ios);
  }
  .resize[data-push='in'][data-nudge='1']::after {
    animation: grip-push-in-again var(--duration-slow) var(--ease-ios);
  }
  /* Two copies of each keyframe set so that a second press at the limit restarts the nudge. */
  @keyframes grip-push-out {
    40% {
      transform: translate(3px, 3px);
    }
  }
  @keyframes grip-push-out-again {
    40% {
      transform: translate(3px, 3px);
    }
  }
  @keyframes grip-push-in {
    40% {
      transform: translate(-3px, -3px);
    }
  }
  @keyframes grip-push-in-again {
    40% {
      transform: translate(-3px, -3px);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .resize::after {
      animation: none !important; /* the tint alone says it */
    }
  }
  .limit-note {
    position: absolute;
    right: 56px; /* beside the grip, over the window's bottom edge: clear of the keys' margin, half in view above the Dock */
    bottom: -13px;
    padding: 4px 12px;
    border-radius: var(--radius-pill);
    background: var(--tint-fill);
    color: var(--on-tint);
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    white-space: nowrap;
    pointer-events: none;
    opacity: 0;
    transition: opacity var(--duration) var(--ease-ios);
    z-index: 30;
  }
  .limit-note.shown {
    opacity: 1;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip: rect(0 0 0 0);
    white-space: nowrap;
  }
  /* The ring goes round the drawn arc, not the 44 px box (most of which is outside the window). */
  .resize:focus-visible {
    outline: none;
  }
  .resize:focus-visible::after {
    outline: 2px solid var(--tint);
    outline-offset: 3px;
  }
</style>
