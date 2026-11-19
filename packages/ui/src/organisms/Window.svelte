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

  Focus: the window itself is focusable (`tabindex="-1"`, so a click on its chrome or a drag of its handle lands
  there). When it is, focus moves on to the element in the content marked `data-window-focus`, if there is one, so a
  window whose keys are handled inside (an app's window-scoped shortcuts) keeps listening after a move or resize.
-->
<script lang="ts">
  import { motion } from '../motion'
  import type { Snippet } from 'svelte'
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
    onframechange?.(
      gesture.kind === 'move'
        ? { ...f, x: f.x + dx, y: Math.max(0, f.y + dy) }
        : { ...f, width: Math.max(minWidth, f.width + dx), height: Math.max(minHeight, f.height + dy) }
    )
  }

  function end(): void {
    gesture = null
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
    <div
      class="resize"
      role="presentation"
      data-testid="{testid}-resize"
      onpointerdown={(e) => {
        e.stopPropagation()
        begin('resize', e)
      }}
      onpointermove={move}
      onpointerup={end}
      onpointercancel={end}
    ></div>
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
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.15),
      -12px 0 40px rgb(0 0 0 / 0.3);
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
  /* iPadOS corner-arc resize affordance */
  .resize {
    position: absolute;
    right: -6px;
    bottom: -6px;
    width: 28px;
    height: 28px;
    cursor: nwse-resize;
    z-index: 20;
  }
  .resize::after {
    content: '';
    position: absolute;
    right: 6px;
    bottom: 6px;
    width: 18px;
    height: 18px;
    border-right: 4px solid transparent;
    border-bottom: 4px solid transparent;
    border-bottom-right-radius: var(--radius-xl);
    transition: border-color var(--duration-fast);
  }
  .ios-window:hover .resize::after {
    border-color: var(--label-tertiary);
  }
</style>
