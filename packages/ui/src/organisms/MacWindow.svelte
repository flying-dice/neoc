<!--
  @organism MacWindow — macOS window frame for the hybrid desktop: traffic
  lights, drag from any element marked `data-drag-region`, corner resize.

  Two content modes:
   • `vibrant` — the frame is transparent so content can use translucent
     sidebars (pair with MacSplitViewTemplate, which supplies drag regions).
   • default ("Catalyst") — an opaque frame hosting an iPadOS-style page,
     optionally scaled like Mac Catalyst, with a thin drag strip on top.

  Atoms: TrafficLights.

  Test ids: `testid` on the window, `${testid}-content`, `${testid}-lights`
  (TrafficLights: `-lights-close|minimize|zoom`), `${testid}-resize`.
-->
<script lang="ts">
  import { motion } from '../motion'
  import type { Snippet } from 'svelte'
  import { scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import TrafficLights from '../atoms/TrafficLights.svelte'

  interface Props {
    testid: string
    title: string
    x?: number
    y?: number
    width?: number
    height?: number
    minWidth?: number
    minHeight?: number
    z?: number
    focused?: boolean
    vibrant?: boolean
    /** Catalyst-style content scale (e.g. 0.85). Ignored for vibrant windows. */
    contentScale?: number
    hidden?: boolean
    onfocus?: () => void
    onclose?: () => void
    onminimize?: () => void
    onzoom?: () => void
    children: Snippet
  }

  let {
    testid,
    title,
    x = $bindable(80),
    y = $bindable(40),
    width = $bindable(820),
    height = $bindable(540),
    minWidth = 420,
    minHeight = 280,
    z = 1,
    focused = true,
    vibrant = false,
    contentScale = 1,
    hidden = false,
    onfocus,
    onclose,
    onminimize,
    onzoom,
    children
  }: Props = $props()

  type Gesture = { kind: 'move' | 'resize'; px: number; py: number; x: number; y: number; w: number; h: number }
  let gesture: Gesture | null = $state(null)

  const INTERACTIVE = 'button, a, input, textarea, select, label, [role="slider"], [contenteditable]'

  function begin(kind: Gesture['kind'], e: PointerEvent, el: HTMLElement): void {
    el.setPointerCapture(e.pointerId)
    gesture = { kind, px: e.clientX, py: e.clientY, x, y, w: width, h: height }
  }

  function onpointerdown(e: PointerEvent): void {
    onfocus?.()
    if (e.button !== 0) return
    const target = e.target as HTMLElement
    if (target.closest(INTERACTIVE)) return
    if (target.closest('[data-drag-region]')) begin('move', e, e.currentTarget as HTMLElement)
  }

  function onpointermove(e: PointerEvent): void {
    if (!gesture) return
    const dx = e.clientX - gesture.px
    const dy = e.clientY - gesture.py
    if (gesture.kind === 'move') {
      x = gesture.x + dx
      y = Math.max(0, gesture.y + dy)
    } else {
      width = Math.max(minWidth, gesture.w + dx)
      height = Math.max(minHeight, gesture.h + dy)
    }
  }

  function end(): void {
    gesture = null
  }
</script>

<div
  class="mac-window"
  data-testid={testid}
  class:focused
  class:vibrant
  class:hidden
  class:dragging={gesture !== null}
  role="dialog"
  tabindex="-1"
  aria-label={title}
  aria-hidden={hidden}
  style:left="{x}px"
  style:top="{y}px"
  style:width="{width}px"
  style:height="{height}px"
  style:z-index={z}
  {onpointerdown}
  {onpointermove}
  onpointerup={end}
  onpointercancel={end}
  transition:scale={{ start: 0.94, duration: motion(240), easing: cubicOut }}
>
  <div class="content" data-testid="{testid}-content" style:zoom={vibrant ? undefined : contentScale}>
    {@render children()}
  </div>

  {#if !vibrant}<div class="drag-strip" data-drag-region></div>{/if}

  <div class="lights">
    <TrafficLights testid="{testid}-lights" active={focused} {onclose} {onminimize} {onzoom} />
  </div>

  <div
    class="resize"
    role="presentation"
    data-testid="{testid}-resize"
    onpointerdown={(e) => {
      e.stopPropagation()
      onfocus?.()
      begin('resize', e, e.currentTarget as HTMLElement)
    }}
    onpointermove={onpointermove}
    onpointerup={end}
  ></div>
</div>

<style>
  .mac-window {
    position: absolute;
    display: flex;
    border-radius: 12px;
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.22),
      inset 0 0 0 0.5px rgb(255 255 255 / 0.12),
      0 10px 28px rgb(0 0 0 / 0.16);
    transition:
      box-shadow var(--duration) var(--ease-ios),
      opacity var(--duration) var(--ease-ios),
      transform var(--duration) var(--ease-ios);
  }
  .focused {
    box-shadow:
      0 0 0 0.5px rgb(0 0 0 / 0.25),
      inset 0 0 0 0.5px rgb(255 255 255 / 0.15),
      0 22px 70px rgb(0 0 0 / 0.35),
      0 8px 20px rgb(0 0 0 / 0.12);
  }
  .vibrant {
    background: transparent;
  }
  .hidden {
    opacity: 0;
    transform: scale(0.2) translateY(120%);
    pointer-events: none;
  }
  .dragging {
    transition: none;
  }
  .content {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    border-radius: inherit;
    /* Catalyst pages: navigation bars clear the traffic lights / drag strip. */
    --nav-top-inset: 8px;
  }
  .content > :global(*) {
    flex: 1;
    min-height: 0;
  }
  .drag-strip {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    height: 16px;
    z-index: 30;
  }
  .lights {
    position: absolute;
    top: 19px;
    left: 19px;
    z-index: 40;
    display: flex;
  }
  .resize {
    position: absolute;
    right: -4px;
    bottom: -4px;
    width: 18px;
    height: 18px;
    cursor: nwse-resize;
    z-index: 40;
  }
</style>
