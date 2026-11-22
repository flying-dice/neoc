/**
 * Who may have a call to the host in flight. Every window of the shell and the shell itself share one WebSocket, and the host
 * answers a connection that has too many requests in flight (`MAX_IN_FLIGHT`, 64) with an error instead of a result. A client
 * that fires a call per item of a list at once must neither be refused for that nor take the slots the shell's own calls and the
 * other windows need, so a client's calls pass through this gate: a few in flight per frame, a fixed number across all frames,
 * the rest wait their turn (frames take turns), and a frame that has an absurd number waiting is told so.
 */

/** Calls in flight on the connection at most, across every frame (the host allows 64 and the shell's own calls need room). */
export const MAX_IN_FLIGHT_TOTAL = 32
/** Calls one frame may have in flight at once. */
export const MAX_IN_FLIGHT_PER_FRAME = 8
/** Calls one frame may have waiting; beyond it a call is refused with a message. */
export const MAX_WAITING_PER_FRAME = 1000

export interface CallGate {
  /**
   * Take a place for one call of `frame` (any object that identifies it): resolves with the function that gives the place
   * back when the call has settled, once the call may start, or `null` at once when the frame already has too many waiting.
   */
  enter(frame: object): Promise<(() => void) | null>
}

interface Limits {
  total: number
  perFrame: number
  maxWaiting: number
}

interface FrameState {
  running: number
  waiting: (() => void)[]
}

export function createCallGate(
  limits: Limits = {
    total: MAX_IN_FLIGHT_TOTAL,
    perFrame: MAX_IN_FLIGHT_PER_FRAME,
    maxWaiting: MAX_WAITING_PER_FRAME
  }
): CallGate {
  const frames = new Map<object, FrameState>()
  let running = 0
  // frames with something waiting, in the order they will be served: each takes a turn, so one frame's burst cannot starve another
  const turns: object[] = []

  const stateOf = (frame: object): FrameState => {
    let state = frames.get(frame)
    if (!state) frames.set(frame, (state = { running: 0, waiting: [] }))
    return state
  }

  function drain(): void {
    for (let tries = turns.length; tries > 0 && running < limits.total; tries--) {
      const frame = turns.shift() as object
      const state = stateOf(frame)
      if (state.running >= limits.perFrame) {
        turns.push(frame)
        continue
      }
      const start = state.waiting.shift()
      if (!start) continue
      if (state.waiting.length > 0) turns.push(frame)
      start()
      tries = turns.length + 1 // something started: look again from the first
    }
  }

  return {
    enter(frame) {
      const state = stateOf(frame)
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
        state.waiting.push(() => {
          state.running += 1
          running += 1
          resolve(release)
        })
        if (first) turns.push(frame)
      })
    }
  }
}

/** The gate the shell's frames share. */
export const callGate: CallGate = createCallGate()
