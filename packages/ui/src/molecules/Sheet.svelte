<!--
  @molecule Sheet — modal sheet presentation: an iPadOS form sheet (centred), a page sheet (bottom, optionally
  with detents small / medium / large the person can drag or step between), or full screen. Focus moves into the
  sheet, Tab stays inside, Escape dismisses (when `dismissible`) and focus returns to where it came from. An
  optional header carries a title with `leading` (Cancel) and `trailing` (Done) actions.
  Atoms: Scrim, Grabber.
  Test ids: `testid` on the sheet, `${testid}-scrim`, `${testid}-grabber`, `${testid}-header`.
-->
<script lang="ts">
  import { describe } from '../aria'
  import { motion } from '../motion'
  import type { Snippet } from 'svelte'
  import { fly } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Scrim from '../atoms/Scrim.svelte'
  import Grabber from '../atoms/Grabber.svelte'
  import { trapFocus } from '../focus'
  import {
    detentHeight,
    nearestDetent,
    normaliseDetents,
    shouldDismiss,
    stepDetent,
    type Detent
  } from '../sheet'

  interface Props {
    testid: string
    open?: boolean
    presentation?: 'form' | 'page' | 'fullScreen'
    width?: number
    /** Tap on the dimmed backdrop / Escape dismisses. */
    dismissible?: boolean
    showGrabber?: boolean
    /** Heights a page sheet can rest at; more than one shows the resize grabber. */
    detents?: Detent[]
    /** The detent it rests at (bindable). Default: the first. */
    detent?: Detent
    /** Accessible name. */
    label?: string
    /** Header title; with `leading` / `trailing` it forms the sheet's bar. */
    title?: string
    leading?: Snippet
    trailing?: Snippet
    onclose?: () => void
    children: Snippet
  }

  let {
    testid,
    open = $bindable(false),
    presentation = 'form',
    width = 540,
    dismissible = true,
    showGrabber = false,
    detents,
    detent = $bindable(),
    label,
    title,
    leading,
    trailing,
    onclose,
    children
  }: Props = $props()

  const stops = $derived(normaliseDetents(detents))
  const current = $derived<Detent>(detent && stops.includes(detent) ? detent : stops[0]!)
  const resizable = $derived(presentation === 'page' && stops.length > 1)
  const grabber = $derived(showGrabber || resizable)

  let layerHeight = $state(0)
  let dragHeight: number | null = $state(null)
  const height = $derived(
    presentation === 'page' && detents !== undefined
      ? layerHeight > 0
        ? (dragHeight ?? detentHeight(current, layerHeight))
        : undefined
      : undefined
  )

  function close(): void {
    open = false
    onclose?.()
  }

  function dismissEscape(): void {
    if (dismissible) close()
  }

  // Resize grabber: drag, click (cycle) or arrow keys.
  let drag: { y: number; start: number; moved: boolean } | null = null
  function ongrabdown(e: PointerEvent): void {
    if (!resizable) return
    ;(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId)
    drag = { y: e.clientY, start: detentHeight(current, layerHeight), moved: false }
  }
  function ongrabmove(e: PointerEvent): void {
    if (!drag) return
    const dy = drag.y - e.clientY
    if (Math.abs(dy) > 4) drag.moved = true
    if (drag.moved) dragHeight = Math.max(80, Math.min(layerHeight, drag.start + dy))
  }
  function ongrabup(): void {
    if (!drag) return
    const { moved } = drag
    drag = null
    if (!moved) {
      detent = stepDetent(stops, current, 1, true)
      return
    }
    const h = dragHeight ?? detentHeight(current, layerHeight)
    dragHeight = null
    if (dismissible && shouldDismiss(h, stops, layerHeight)) close()
    else detent = nearestDetent(stops, h, layerHeight)
  }
  function ongrabkey(e: KeyboardEvent): void {
    if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
      e.preventDefault()
      detent = stepDetent(stops, current, e.key === 'ArrowUp' ? 1 : -1)
    }
  }
</script>

{#if open}
  <div class="ios-sheet-layer {presentation}" bind:clientHeight={layerHeight}>
    <Scrim testid="{testid}-scrim" onclick={() => dismissible && close()} />
    <div
      class="sheet"
      role="dialog"
      data-testid={testid}
      aria-modal="true"
      aria-label={label ?? title}
      style:--sheet-width="{width}px"
      style:height={height === undefined ? undefined : `${height}px`}
      {@attach trapFocus({ onescape: dismissEscape, fallback: 'container' })}
      transition:fly={{ y: 700, duration: motion(450), easing: cubicOut, opacity: 1 }}
    >
      {#if grabber}
        <div class="grabber">
          {#if resizable}
            <button
              type="button"
              class="grab"
              data-testid="{testid}-grabber"
              aria-label="Resize sheet"
              {...describe('Arrow up and down change the height; Enter cycles it')}
              onpointerdown={ongrabdown}
              onpointermove={ongrabmove}
              onpointerup={ongrabup}
              onpointercancel={ongrabup}
              onkeydown={ongrabkey}
            >
              <Grabber />
            </button>
          {:else}
            <Grabber />
          {/if}
        </div>
      {/if}
      {#if title || leading || trailing}
        <header class="bar" class:padded={grabber} data-testid="{testid}-header">
          <span class="side start">{@render leading?.()}</span>
          <h2 class="title">{title ?? ''}</h2>
          <span class="side end">{@render trailing?.()}</span>
        </header>
      {/if}
      {#if title || leading || trailing}
        <div class="content">{@render children()}</div>
      {:else}
        {@render children()}
      {/if}
    </div>
  </div>
{/if}

<style>
  .ios-sheet-layer {
    position: absolute;
    inset: 0;
    z-index: 1000;
    display: flex;
    justify-content: center;
  }
  .form {
    align-items: center;
  }
  .page {
    align-items: flex-end;
  }
  .fullScreen {
    align-items: stretch;
  }
  .sheet {
    position: relative;
    display: flex;
    flex-direction: column;
    overflow: hidden;
    background: var(--bg-grouped-elevated);
    color: var(--label);
    box-shadow: var(--shadow-popover);
    outline: 0;
  }
  .form .sheet {
    width: min(var(--sheet-width), calc(100% - 40px));
    max-height: min(620px, calc(100% - 80px));
    border-radius: 13px;
  }
  .page .sheet {
    width: 100%;
    max-height: calc(100% - 44px);
    border-radius: 13px 13px 0 0;
    transition: height var(--duration) var(--ease-ios);
  }
  .fullScreen .sheet {
    width: 100%;
    border-radius: 0;
  }
  .grabber {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 1;
  }
  .grab {
    appearance: none;
    display: block;
    width: 100%;
    height: 24px;
    padding: 0;
    border: 0;
    background: transparent;
    cursor: grab;
    touch-action: none;
  }
  .grab:focus-visible {
    outline-offset: -3px;
  }
  .bar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 8px;
    min-height: 52px;
    padding: 0 12px;
    border-bottom: var(--hairline) solid var(--separator);
    flex: none;
  }
  .bar.padded {
    padding-top: 12px;
  }
  .side {
    display: flex;
    min-width: 0;
  }
  .end {
    justify-content: flex-end;
  }
  .title {
    margin: 0;
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
    text-align: center;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    max-width: 100%;
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: auto;
    display: flex;
    flex-direction: column;
  }
</style>
