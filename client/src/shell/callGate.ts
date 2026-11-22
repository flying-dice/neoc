/**
 * Who may have a call to the host in flight. Every window of the shell and the shell itself share one WebSocket, and the host
 * answers a connection that has too many requests in flight (`MAX_IN_FLIGHT`, 64) with an error instead of a result. A client
 * that fires a call per item of a list at once must neither be refused for that nor take the slots the shell's own calls and the
 * other windows need, so a client's calls pass through this gate: a few in flight per frame, a fixed number across all frames,
 * the rest wait their turn (frames take turns), and a frame that has an absurd number waiting is told so.
 *
 * A place stands for a request the host is still running: it is given back when the host has answered it or the connection
 * has gone, never because the client stopped waiting (the caller sets no timeout of its own). So what is in flight on the
 * host for one key never exceeds `perFrame` and across keys never exceeds `total`, whatever the clients do, and a handler
 * that never returns takes the places of its own app only. A key is an app, not a window: closing and reopening the window
 * of an app whose handlers hang finds the same places still taken, because the host is still running them.
 */

/** Calls in flight on the connection at most, across every frame (the host allows 64 and the shell's own calls need room). */
export const MAX_IN_FLIGHT_TOTAL = 32
/** Calls one frame may have in flight at once. */
export const MAX_IN_FLIGHT_PER_FRAME = 8
/** Calls one frame may have waiting; beyond it a call is refused with a message. */
export const MAX_WAITING_PER_FRAME = 1000

export interface CallGate {
  /**
   * Take a place for one call of `frame` (anything that identifies it: the app): resolves with the function that gives the
   * place back when the host has answered the call, once the call may start, or `null` at once when the frame already has too
   * many waiting. A call that is still waiting when `gone()` turns true (its window closed) never starts: it resolves with a
   * function that gives back nothing, and stops counting as waiting.
   */
  enter(frame: string | object, gone?: () => boolean): Promise<(() => void) | null>
}

interface Limits {
  total: number
  perFrame: number
  maxWaiting: number
}

interface Waiting {
  start(): void
  /** The call was given up while it waited (its window closed). */
  gone(): boolean
  forget(): void
}

interface FrameState {
  running: number
  waiting: Waiting[]
}

export function createCallGate(
  limits: Limits = {
    total: MAX_IN_FLIGHT_TOTAL,
    perFrame: MAX_IN_FLIGHT_PER_FRAME,
    maxWaiting: MAX_WAITING_PER_FRAME
  }
): CallGate {
  const frames = new Map<string | object, FrameState>()
  let running = 0
  // frames with something waiting, in the order they will be served: each takes a turn, so one frame's burst cannot starve another
  const turns: (string | object)[] = []

  const stateOf = (frame: string | object): FrameState => {
    let state = frames.get(frame)
    if (!state) frames.set(frame, (state = { running: 0, waiting: [] }))
    return state
  }

  function drain(): void {
    for (let tries = turns.length; tries > 0 && running < limits.total; tries--) {
      const frame = turns.shift() as string | object
      const state = stateOf(frame)
      while (state.waiting[0]?.gone()) state.waiting.shift()?.forget()
      if (state.running >= limits.perFrame) {
        turns.push(frame)
        continue
      }
      const next = state.waiting.shift()
      if (!next) {
        if (state.running === 0) frames.delete(frame)
        continue
      }
      if (state.waiting.length > 0) turns.push(frame)
      next.start()
      tries = turns.length + 1 // something started: look again from the first
    }
  }

  return {
    enter(frame, gone = () => false) {
      const state = stateOf(frame)
      // calls whose window has closed no longer wait: they do not count against the key's waiting limit
      if (state.waiting.length > 0) {
        for (let i = state.waiting.length - 1; i >= 0; i--) {
          if (state.waiting[i]?.gone()) state.waiting.splice(i, 1)[0]?.forget()
        }
      }
      let given = false
      const release = (): void => {
        if (given) return
        given = true
        state.running -= 1
        running -= 1
        if (state.running === 0 && state.waiting.length === 0) frames.delete(frame)
        drain()
      }
      if (state.running < limits.perFrame && running < limits.total && state.waiting.length === 0) {
        state.running += 1
        running += 1
        return Promise.resolve(release)
      }
      if (state.waiting.length >= limits.maxWaiting) return Promise.resolve(null)
      return new Promise((resolve) => {
        const first = state.waiting.length === 0
        state.waiting.push({
          start() {
            state.running += 1
            running += 1
            resolve(release)
          },
          gone,
          // never started, so it holds no place: the caller's `release()` of it must not give one back
          forget: () => resolve(() => undefined)
        })
        if (first) turns.push(frame)
      })
    }
  }
}

/** The gate the shell's frames share. */
export const callGate: CallGate = createCallGate()
