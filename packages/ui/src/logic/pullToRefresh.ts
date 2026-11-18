/**
 * Pull to refresh as a Svelte action. A downward touch drag that starts with the scroller at the top pulls the
 * content down with resistance; releasing past the threshold calls `onrefresh`. The component shows the
 * indicator from `onchange`. Touch only: a mouse has the keyboard-reachable "Refresh" button instead.
 */
import { PULL_THRESHOLD, pullArmed, pullDistance } from './gestures'

export interface PullOptions {
  enabled: boolean
  /** Resolves when the refresh is done (the spinner stays until then). */
  onrefresh: () => void | Promise<void>
  /** The distance pulled (0 = idle) and whether a refresh is running. */
  onchange?: (distance: number, refreshing: boolean) => void
  /** The scrolling element whose `scrollTop` must be 0 (default: the node itself). */
  scroller?: () => HTMLElement | null
}

export function pullToRefresh(node: HTMLElement, initial: PullOptions) {
  let opts = initial
  let startY = 0
  let tracking = false
  let pulled = 0
  let refreshing = false

  const scroller = (): HTMLElement => opts.scroller?.() ?? node
  const report = (d: number): void => opts.onchange?.(d, refreshing)

  function start(e: TouchEvent): void {
    if (!opts.enabled || refreshing || e.touches.length !== 1) return
    if (scroller().scrollTop > 0) return
    startY = e.touches[0].clientY
    tracking = true
    pulled = 0
  }

  function move(e: TouchEvent): void {
    if (!tracking) return
    const dy = e.touches[0].clientY - startY
    if (dy <= 0) {
      if (pulled > 0) report((pulled = 0))
      return
    }
    if (scroller().scrollTop > 0) return void (tracking = false)
    pulled = pullDistance(dy)
    if (e.cancelable) e.preventDefault() // own the gesture: no native overscroll bounce
    report(pulled)
  }

  async function end(): Promise<void> {
    if (!tracking) return
    tracking = false
    const armed = pullArmed(pulled)
    pulled = 0
    if (!armed) return report(0)
    refreshing = true
    report(PULL_THRESHOLD)
    try {
      await opts.onrefresh()
    } finally {
      refreshing = false
      report(0)
    }
  }

  node.addEventListener('touchstart', start, { passive: true })
  node.addEventListener('touchmove', move, { passive: false })
  node.addEventListener('touchend', end)
  node.addEventListener('touchcancel', end)
  return {
    update(next: PullOptions) {
      opts = next
    },
    destroy() {
      node.removeEventListener('touchstart', start)
      node.removeEventListener('touchmove', move)
      node.removeEventListener('touchend', end)
      node.removeEventListener('touchcancel', end)
    }
  }
}
