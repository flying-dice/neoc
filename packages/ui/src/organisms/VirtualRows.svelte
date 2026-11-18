<!--
  @organism VirtualRows — renders only the rows near the viewport of a list of `total` rows (a few dozen DOM rows
  for 100 000), inside the list's own scroll container: the nearest scrolling ancestor, or itself when none
  scrolls. A spacer above and below keeps the scrollbar honest. Row heights are measured, so rows may wrap.
  `onrange` reports the rows rendered (the caller asks its host for that window); `keys` and `from` (the rows it
  has) anchor scrolling on the first visible row, so rows added above do not move what you are reading.
  Test id: `testid`. Content is the `row` snippet, called with the absolute row index.
-->
<script lang="ts">
  import { tick, untrack, type Snippet } from 'svelte'
  import { offsetInScroller, perFrame, scrollParent } from '../logic/dom'
  import { anchorOf, anchorShift, HeightIndex, visibleRange, type Anchor, type RowRange } from '../logic/virtual'

  interface Props {
    testid: string
    total: number
    /** The row height in px until a row is measured. */
    estimate?: number
    overscan?: number
    /** The keys of the rows the caller holds, starting at absolute row `from` (scroll anchoring). */
    keys?: readonly (string | number)[]
    from?: number
    onrange?: (range: RowRange) => void
    row: Snippet<[number]>
  }

  let { testid, total, estimate = 52, overscan = 6, keys = [], from = 0, onrange, row }: Props = $props()

  const heights = new HeightIndex(0, untrack(() => estimate))
  let host: HTMLElement | undefined = $state()
  let scroller: HTMLElement | null = $state(null)
  let self = $state(false)
  let scrollTop = $state(0)
  let viewportHeight = $state(0)
  let listOffset = $state(0)
  /** When the list scrolls itself it is as tall as the space left in its clipping container. */
  let selfHeight = $state(0)
  /** Bumped when a measurement changes the height table (the table itself is not reactive). */
  let version = $state(0)

  const range = $derived.by(() => {
    void version
    heights.setTotal(total)
    return visibleRange(heights, { scrollTop, height: viewportHeight, listOffset }, overscan)
  })
  const padTop = $derived.by(() => (void version, heights.offsetOf(range.start)))
  const padBottom = $derived.by(
    () => (void version, Math.max(0, heights.totalHeight() - heights.offsetOf(range.end)))
  )
  const indices = $derived(Array.from({ length: Math.max(0, range.end - range.start) }, (_, i) => range.start + i))

  // ── find the scroller and follow it ──
  $effect(() => {
    if (!host) return undefined
    const found = scrollParent(host)
    self = found === null
    const el = found ?? host
    scroller = el
    let clip: HTMLElement | null = null
    if (self) {
      for (let n = host.parentElement; n && !clip; n = n.parentElement)
        if (getComputedStyle(n).overflowY === 'hidden') clip = n
    }
    const read = perFrame((): void => {
      if (self && clip && host)
        selfHeight = Math.max(120, Math.floor(clip.getBoundingClientRect().bottom - host.getBoundingClientRect().top))
      scrollTop = el.scrollTop
      viewportHeight = el.clientHeight
      listOffset = el === host ? 0 : offsetInScroller(host!, el)
    })
    read()
    el.addEventListener('scroll', read, { passive: true })
    const ro = new ResizeObserver(read)
    ro.observe(el)
    ro.observe(host)
    if (clip) ro.observe(clip)
    return () => {
      el.removeEventListener('scroll', read)
      ro.disconnect()
    }
  })

  // ── measure rows ──
  const bump = perFrame(() => void version++)
  let calibrated = false
  const sizes =
    typeof ResizeObserver === 'undefined'
      ? null
      : new ResizeObserver((entries) => {
          for (const e of entries) {
            const index = Number((e.target as HTMLElement).dataset.index)
            const h = e.borderBoxSize?.[0]?.blockSize ?? (e.target as HTMLElement).offsetHeight
            if (!calibrated && h > 0) {
              calibrated = true
              heights.setEstimate(h) // the first real row is the best guess for the rest
            }
            if (heights.set(index, h)) bump()
          }
          bump()
        })
  const measure = (el: HTMLElement): (() => void) => {
    sizes?.observe(el)
    return () => sizes?.unobserve(el)
  }
  $effect(() => () => sizes?.disconnect())

  $effect(() => {
    onrange?.(range)
  })

  // ── scroll anchoring: hold the first visible row still while rows change above it ──
  let held: { anchor: Anchor<string | number>; offset: number } | null = null
  $effect.pre(() => {
    const now = { keys, from }
    if (held && scroller && scrollTop > 0) {
      const shift = anchorShift(held.anchor, now.keys, now.from, heights, held.offset)
      if (shift !== null) {
        const el = scroller
        void tick().then(() => (el.scrollTop += shift))
      }
    }
    const first = untrack(() => heights.indexAt(Math.max(0, scrollTop - listOffset)))
    const anchor = anchorOf(now.keys, now.from, first)
    held = anchor ? { anchor, offset: untrack(() => heights.offsetOf(anchor.index)) } : null
  })

  /** Bring row `index` to the top of the viewport (rows outside the DOM are reached by scrolling to them). */
  export function scrollToIndex(index: number, align: 'start' | 'end' = 'start'): void {
    if (!scroller) return
    const i = Math.min(Math.max(0, index), Math.max(0, total - 1))
    const top = listOffset + heights.offsetOf(i)
    scroller.scrollTop = align === 'start' ? top : top + heights.heightOf(i) - scroller.clientHeight
  }
</script>

<div
  class="ios-virtual"
  class:self
  data-testid={testid}
  bind:this={host}
  style:height={self && selfHeight ? `${selfHeight}px` : undefined}
>
  <div class="rows" style:padding-top="{padTop}px" style:padding-bottom="{padBottom}px">
    {#each indices as i (i)}
      <div class="vrow" data-index={i} {@attach measure}>{@render row(i)}</div>
    {/each}
  </div>
</div>

<style>
  .ios-virtual {
    min-width: 0;
  }
  /* No scrolling ancestor: the list scrolls itself, filling the space it is given. */
  .self {
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior: contain;
  }
  .rows {
    min-width: 0;
  }
  .vrow {
    min-width: 0;
  }
</style>
