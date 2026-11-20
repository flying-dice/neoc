/** Small DOM helpers shared by the kit's scrolling components. */

/**
 * The nearest ancestor that scrolls vertically; null when none does. A clipping container (`overflow: hidden`,
 * like a rounded list section) does not scroll, so it is skipped unless `stopAtClip`: a navigation bar in a sheet
 * or preview that clips its content wants a static bar, not the scroller outside. `mustScroll` also requires that
 * the content overflows it (it is bounded, so it really scrolls).
 */
export function scrollParent(el: HTMLElement, stopAtClip = false, mustScroll = false): HTMLElement | null {
  for (let node = el.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    // `mustScroll`: an auto-height box (a body that grows with its content) has the overflow style but no bounds.
    if ((overflowY === 'auto' || overflowY === 'scroll') && (!mustScroll || node.scrollHeight > node.clientHeight + 1))
      return node
    if (stopAtClip && overflowY === 'hidden') return null
  }
  return null
}

/** The outermost ancestor that clips its content: the box that bounds a list which has no scroller of its own. */
export function outerClip(el: HTMLElement): HTMLElement | null {
  let clip: HTMLElement | null = null
  for (let node = el.parentElement; node; node = node.parentElement)
    if (getComputedStyle(node).overflowY === 'hidden') clip = node
  return clip
}

/** Where `el` starts inside `scroller`'s scrollable content. */
export function offsetInScroller(el: HTMLElement, scroller: HTMLElement): number {
  return el.getBoundingClientRect().top - scroller.getBoundingClientRect().top + scroller.scrollTop
}

/** Run `fn` at most once per animation frame, with the last arguments. */
export function perFrame<A extends unknown[]>(fn: (...args: A) => void): (...args: A) => void {
  let queued = 0
  let latest: A
  return (...args: A) => {
    latest = args
    if (queued) return
    queued = requestAnimationFrame(() => {
      queued = 0
      fn(...latest)
    })
  }
}
