/**
 * A manual clock for tests and Storybook plays: timers fire only when `advance` moves time, in due order (ties in
 * the order they were set), and every fired timer is followed by a macrotask turn so promise chains it started
 * (a debounced write and its ack) settle before the next timer. Test support: nothing in production imports it.
 */
import type { TimerHandle, Timers } from '../core/timers'

interface Scheduled {
  id: number
  at: number
  fn: () => void
}

/** Let every pending promise continuation run. */
export const turn = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0))

export class FakeTimers implements Timers {
  private clock: number
  private seq = 0
  private queue: Scheduled[] = []

  constructor(start = Date.UTC(2026, 10, 18, 9, 0, 0)) {
    this.clock = start
  }

  now(): number {
    return this.clock
  }

  setTimeout(fn: () => void, ms: number): TimerHandle {
    const item: Scheduled = { id: ++this.seq, at: this.clock + Math.max(0, ms), fn }
    this.queue.push(item)
    return item.id
  }

  clearTimeout(handle: TimerHandle | undefined): void {
    if (handle === undefined) return
    this.queue = this.queue.filter((t) => t.id !== handle)
  }

  /** Number of timers waiting. */
  get pending(): number {
    return this.queue.length
  }

  /** Move time forward `ms`, firing what comes due and letting promises settle after each. */
  async advance(ms: number): Promise<void> {
    const target = this.clock + ms
    await turn()
    for (;;) {
      const next = this.queue.filter((t) => t.at <= target).sort((a, b) => a.at - b.at || a.id - b.id)[0]
      if (!next) break
      this.queue = this.queue.filter((t) => t !== next)
      this.clock = Math.max(this.clock, next.at)
      next.fn()
      await turn()
    }
    this.clock = target
    await turn()
  }

  /** Let promises settle without moving time. */
  settle(): Promise<void> {
    return turn()
  }
}
