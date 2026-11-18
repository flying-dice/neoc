/** Small DOM helpers shared by the kit's scrolling components. */

/** The nearest ancestor that scrolls vertically; null when none does (or a clipped container comes first). */
export function scrollParent(el: HTMLElement): HTMLElement | null {
  for (let node = el.parentElement; node; node = node.parentElement) {
    const { overflowY } = getComputedStyle(node)
    if (overflowY === 'auto' || overflowY === 'scroll') return node
    if (overflowY === 'hidden') return null
  }
  return null
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
