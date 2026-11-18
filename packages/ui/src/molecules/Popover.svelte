<!--
  @molecule Popover — a floating surface anchored to an element (or a point), with an optional arrow. It prefers
  the requested edge, flips when there is no room and never leaves its container (the nearest positioned
  ancestor, the window). Light dismiss: Escape, or a press outside it (a press on the anchor is the anchor's own
  business, so a trigger can toggle). On a compact container (`compact`) it becomes a bottom sheet instead, with
  no arrow. `dialog` (default) traps focus and restores it; `menu` leaves focus to its content (a menu focuses
  itself). Test ids: `testid` on the surface, `${testid}-arrow`, `${testid}-scrim` (compact).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { fade, fly } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import { trapFocus } from '../focus'
  import { place, type Edge, type Placement } from '../floating'
  import { motion } from '../motion'

  interface Props {
    testid: string
    open?: boolean
    /** The element to sit next to. */
    anchor?: HTMLElement | null
    /** Or a point in viewport coordinates (a context menu). */
    point?: { x: number; y: number } | null
    edge?: Edge
    arrow?: boolean
    /** Collapse to a bottom sheet (narrow window). */
    compact?: boolean
    /** Accessible name. */
    label?: string
    /** `dialog`: role dialog, focus trapped and restored. `menu`: no role or trap; the content manages focus. */
    kind?: 'dialog' | 'menu'
    /** Dismiss on a press outside (default true). */
    lightDismiss?: boolean
    onclose?: () => void
    children: Snippet
  }

  let {
    testid,
    open = $bindable(false),
    anchor = null,
    point = null,
    edge = 'bottom',
    arrow = true,
    compact = false,
    label,
    kind = 'dialog',
    lightDismiss = true,
    onclose,
    children
  }: Props = $props()

  let surface: HTMLElement | undefined = $state()
  let placement: Placement | null = $state(null)

  function close(): void {
    open = false
    onclose?.()
  }

  /** The surface's containing block (the window root): coordinates are relative to it. */
  function container(): HTMLElement | null {
    return surface?.offsetParent instanceof HTMLElement ? surface.offsetParent : null
  }

  function reposition(): void {
    const el = surface
    const host = container()
    if (!el || !host || compact) return
    const box = host.getBoundingClientRect()
    const target = anchor?.getBoundingClientRect()
    const rect = target
      ? { x: target.left - box.left, y: target.top - box.top, width: target.width, height: target.height }
      : point
        ? { x: point.x - box.left, y: point.y - box.top, width: 0, height: 0 }
        : { x: box.width / 2, y: box.height / 2, width: 0, height: 0 }
    placement = place(
      rect,
      { width: el.offsetWidth, height: el.offsetHeight },
      { width: host.clientWidth, height: host.clientHeight },
      edge,
      arrow && target ? 12 : 6,
      8,
      getComputedStyle(el).direction === 'rtl'
    )
  }

  $effect(() => {
    if (!open || !surface) return undefined
    reposition()
    const host = container()
    const ro = new ResizeObserver(reposition)
    ro.observe(surface)
    if (host) ro.observe(host)
    if (anchor) ro.observe(anchor)
    return () => {
      ro.disconnect()
      placement = null
    }
  })

  // A press outside dismisses; one on the anchor does not (the anchor toggles).
  $effect(() => {
    if (!open || !lightDismiss) return undefined
    const onpointerdown = (e: PointerEvent): void => {
      const t = e.target as Node
      if (surface?.contains(t) || anchor?.contains(t)) return
      close()
    }
    document.addEventListener('pointerdown', onpointerdown, true)
    return () => document.removeEventListener('pointerdown', onpointerdown, true)
  })

  $effect(() => {
    if (!open) return undefined
    const onescape = (e: KeyboardEvent): void => {
      if (e.key === 'Escape' && !e.defaultPrevented) close()
    }
    document.addEventListener('keydown', onescape)
    return () => document.removeEventListener('keydown', onescape)
  })

  function onkeydown(e: KeyboardEvent): void {
    if (e.key === 'Escape') {
      e.stopPropagation()
      e.preventDefault()
      close()
    }
  }

  const arrowStyle = $derived.by(() => {
    if (!placement || !arrow || !anchor || compact) return undefined
    const vertical = placement.edge === 'bottom' || placement.edge === 'top'
    return vertical ? `left:${placement.arrow}px` : `top:${placement.arrow}px`
  })
</script>

{#if open}
  {#if compact}
    <div class="layer compact">
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <div class="scrim" data-testid="{testid}-scrim" transition:fade={{ duration: motion(200) }} onclick={() => lightDismiss && close()}></div>
      <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
      <div
        class="surface sheet"
        role={kind === 'dialog' ? 'dialog' : undefined}
        aria-modal={kind === 'dialog' ? 'true' : undefined}
        aria-label={label}
        data-testid={testid}
        bind:this={surface}
        {@attach kind === 'dialog' ? trapFocus() : () => {}}
        {onkeydown}
        transition:fly={{ y: 300, duration: motion(300), easing: cubicOut, opacity: 1 }}
      >
        {@render children()}
      </div>
    </div>
  {:else}
    <!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
    <div
      class="surface popover {placement?.edge ?? edge}"
      class:placed={placement !== null}
      role={kind === 'dialog' ? 'dialog' : undefined}
      aria-label={label}
      data-testid={testid}
      style:left="{placement?.x ?? 0}px"
      style:top="{placement?.y ?? 0}px"
      style:max-width="{placement?.maxWidth ?? 320}px"
      style:max-height="{placement?.maxHeight ?? 320}px"
      bind:this={surface}
      {@attach kind === 'dialog' ? trapFocus() : () => {}}
      {onkeydown}
      transition:fade={{ duration: motion(120) }}
    >
      {#if arrow && anchor}<span class="arrow" data-testid="{testid}-arrow" aria-hidden="true" style={arrowStyle}></span>{/if}
      <div class="body">{@render children()}</div>
    </div>
  {/if}
{/if}

<style>
  .layer {
    position: absolute;
    inset: 0;
    z-index: 1050;
  }
  .scrim {
    position: absolute;
    inset: 0;
    background: var(--dimming);
  }
  .surface {
    background: var(--bg-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    outline: 0;
  }
  .popover {
    position: absolute;
    z-index: 1050;
    border-radius: var(--radius-lg);
    visibility: hidden; /* until measured and placed, so it does not flash at the origin */
  }
  .popover.placed {
    visibility: visible;
  }
  .body {
    border-radius: inherit;
    overflow: auto;
    max-height: inherit;
    max-width: inherit;
  }
  .sheet {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    max-height: calc(100% - 44px);
    overflow: auto;
    border-radius: var(--radius-lg) var(--radius-lg) 0 0;
    padding-bottom: 8px;
  }
  .arrow {
    position: absolute;
    width: 14px;
    height: 14px;
    background: var(--bg-elevated);
    transform: translate(-50%, -50%) rotate(45deg);
    border-radius: 3px 0 0 0;
  }
  .bottom > .arrow {
    top: 0;
  }
  .top > .arrow {
    bottom: 0;
    top: auto;
    transform: translate(-50%, 50%) rotate(225deg);
  }
  .leading > .arrow {
    right: 0;
    transform: translate(50%, -50%) rotate(135deg);
  }
  .trailing > .arrow {
    left: 0;
    transform: translate(-50%, -50%) rotate(-45deg);
  }
</style>
