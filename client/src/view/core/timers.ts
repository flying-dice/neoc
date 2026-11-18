/**
 * An injectable clock. Everything in `core/` that waits (debounce, maxWait, source windows, reconnect, toast
 * expiry) takes `Timers`, so tests drive time deterministically (`host/clock.ts`) and production uses the real one.
 */
export type TimerHandle = unknown

export interface Timers {
  setTimeout(fn: () => void, ms: number): TimerHandle
  clearTimeout(handle: TimerHandle | undefined): void
  /** Epoch ms. */
  now(): number
}

export const realTimers: Timers = {
  setTimeout: (fn, ms) => setTimeout(fn, ms),
  clearTimeout: (h) => clearTimeout(h as ReturnType<typeof setTimeout> | undefined),
  now: () => Date.now()
}
