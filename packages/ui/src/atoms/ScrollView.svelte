<!--
  @atom ScrollView — a scrolling region (vertical, horizontal or both) with optional scroll snapping, hidden
  indicators, a throttled `onscroll` and pull to refresh. It is a focusable `region` so the keyboard can scroll it.
  With `refresh`, a touch pull-down refreshes, and a "Refresh" button appears on focus for keyboard and screen
  reader users. Test ids: `testid` on the region, `${testid}-refresh` on the button.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { PULL_THRESHOLD } from '../logic/gestures'
  import { pullToRefresh } from '../logic/pullToRefresh'

  interface Props {
    testid: string
    axis?: 'v' | 'h' | 'both'
    indicators?: boolean
    snap?: boolean
    label?: string
    /** Enables pull to refresh and the keyboard "Refresh" button. */
    refresh?: boolean
    /** Resolves when the refresh finished; the spinner shows until then. */
    onrefresh?: () => void | Promise<void>
    onscroll?: (position: { x: number; y: number }) => void
    /** The scrolling element (for scroll-to and virtualization). */
    element?: HTMLElement
    style?: string
    children: Snippet
  }

  let {
    testid,
    axis = 'v',
    indicators = true,
    snap = false,
    label = 'Scrollable content',
    refresh = false,
    onrefresh,
    onscroll,
    element = $bindable(),
    style,
    children
  }: Props = $props()

  let pulled = $state(0)
  let refreshing = $state(false)
  let frame = 0

  function scrolled(e: Event): void {
    if (!onscroll || frame) return
    const el = e.currentTarget as HTMLElement
    frame = requestAnimationFrame(() => {
      frame = 0
      onscroll({ x: Math.round(el.scrollLeft), y: Math.round(el.scrollTop) })
    })
  }

  async function run(): Promise<void> {
    if (refreshing) return
    refreshing = true
    try {
      await onrefresh?.()
    } finally {
      refreshing = false
    }
  }
</script>

<!-- svelte-ignore a11y_no_noninteractive_tabindex (a scrollable region must be reachable by keyboard) -->
<div
  class="ios-scroll"
  class:both={axis === 'both'}
  class:horizontal={axis === 'h'}
  class:snap
  class:bare={!indicators}
  data-testid={testid}
  role="region"
  aria-label={label}
  aria-busy={refreshing || undefined}
  tabindex="0"
  bind:this={element}
  onscroll={scrolled}
  use:pullToRefresh={{
    enabled: refresh && !!onrefresh,
    onrefresh: run,
    onchange: (d, r) => {
      pulled = d
      refreshing = r
    }
  }}
  {style}
>
  {#if refresh && onrefresh}
    <button type="button" class="refresh-key" data-testid="{testid}-refresh" onclick={run} disabled={refreshing}>
      Refresh
    </button>
    <div class="pull" style:height="{refreshing ? PULL_THRESHOLD : pulled}px" aria-hidden="true">
      {#if pulled > 0 || refreshing}
        <span class="spinner" class:spin={refreshing} style:opacity={Math.min(1, pulled / PULL_THRESHOLD)}></span>
      {/if}
    </div>
  {/if}
  {@render children()}
</div>

<style>
  .ios-scroll {
    min-width: 0;
    min-height: 0;
    overflow-y: auto;
    overflow-x: hidden;
    overscroll-behavior: contain;
    -webkit-overflow-scrolling: touch;
  }
  .horizontal {
    display: flex;
    overflow-x: auto;
    overflow-y: hidden;
  }
  .both {
    overflow: auto;
  }
  .snap {
    scroll-snap-type: x mandatory;
  }
  .snap > :global(*) {
    scroll-snap-align: start;
  }
  .bare {
    scrollbar-width: none;
  }
  .bare::-webkit-scrollbar {
    display: none;
  }
  .ios-scroll:focus-visible {
    outline: 3px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .pull {
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    transition: height var(--duration-fast) var(--ease-ios);
  }
  .spinner {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid var(--label-quaternary);
    border-top-color: var(--label-secondary);
  }
  .spin {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* The keyboard path: hidden until focused, then the first thing on the screen. */
  .refresh-key {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    padding: 0;
    border: 0;
  }
  .refresh-key:focus-visible {
    position: sticky;
    top: 4px;
    z-index: 5;
    width: auto;
    height: auto;
    margin: 4px auto;
    padding: 8px 16px;
    clip-path: none;
    border-radius: 999px;
    background: var(--tint-fill);
    color: #fff;
    font: var(--type-subheadline);
    font-weight: 600;
    outline: 3px solid var(--focus-ring);
    outline-offset: 2px;
  }
  @media (prefers-reduced-motion: reduce) {
    .pull {
      transition: none;
    }
    .spin {
      animation-duration: 2s !important; /* it says the app is busy: slower, but turning (base.css stops the rest) */
    }
  }
</style>
