import type { Api, Exit } from '@terminal/rpc'
import type { Host } from '@vos/host'

/** What the window needs of the backend: a session, what is typed into it, and what it missed. */
export type TerminalRpc = Pick<Api, 'open' | 'write' | 'resize' | 'ack' | 'replay' | 'close'>
/** Where the backend's events are heard (`host.on`). */
export type TerminalEvents = Pick<Host, 'on'>

/** What draws the output: the terminal emulator on the page (xterm.js), or a recorder in a test. */
export interface Screen {
  /** Draw these bytes; `drawn` is called once they are on the screen. */
  write(bytes: Uint8Array, drawn: () => void): void
  /** Clear everything: a new session starts on an empty screen. */
  reset(): void
}

/**
 * Where a window's session is:
 *  - `starting`: the shell was asked for and has not answered;
 *  - `running`: there is a shell, and what is typed goes to it;
 *  - `exited`: the shell ended by itself (`exit` says how);
 *  - `ended`: the backend no longer has the session (the window was away for too long, or the app was restarted);
 *  - `failed`: no shell could be started (`failure` says why).
 */
export type Phase = 'starting' | 'running' | 'exited' | 'ended' | 'failed'

interface OutputEvent {
  session: string
  offset: number
  data: string
}
interface ExitEvent {
  session: string
  offset: number
  code: number
  signal?: string
}

/** The most bytes of input in one `write` (the backend takes 64 KiB). */
export const WRITE_BYTES = 48 * 1024
/** The most input that waits for the shell; what is typed past it is dropped (the shell is not reading). */
export const PENDING_BYTES = 1024 * 1024
/** The page says what it has drawn once this much is unacknowledged (a quarter of the backend's window). */
export const ACK_BYTES = 64 * 1024
/** How often the page says it is still there, in milliseconds (the backend ends a session it has not heard for 3 minutes). */
export const PING_MS = 15_000

/** The backend's own refusal (`ctx:fail`), as opposed to a call that never reached it. */
const APPLICATION_ERROR = -32010
const refused = (e: unknown): boolean => (e as { code?: unknown } | null)?.code === APPLICATION_ERROR
const messageOf = (e: unknown): string => (e instanceof Error ? e.message : String(e))

const encoder = new TextEncoder()

export function toBase64(bytes: Uint8Array): string {
  let binary = ''
  for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000))
  return btoa(binary)
}

export function fromBase64(text: string): Uint8Array {
  const binary = atob(text)
  const bytes = new Uint8Array(binary.length)
  for (let i = 0; i < binary.length; i++) bytes[i] = binary.charCodeAt(i)
  return bytes
}

/**
 * One window's terminal session: the shell the backend runs for it, the output that reaches the screen in order and
 * whole, and what is typed. The backend's events reach every window of the app, so everything here is filtered by the
 * id of the session this window opened.
 */
export class Terminal {
  #phase = $state<Phase>('starting')
  #exit = $state.raw<Exit | null>(null)
  // why no shell could be started
  #failure = $state<string | null>(null)
  // why the last call did not reach the backend, while there is a session (the host connection is away)
  #problem = $state<string | null>(null)

  readonly #rpc: TerminalRpc
  readonly #events: TerminalEvents
  #screen: Screen | null = null
  #session: string | null = null
  #size = { rows: 24, cols: 80 }
  // the size the backend was last told, and whether a resize is on its way
  #told = { rows: 0, cols: 0 }
  #resizing = false
  // output: the offset of the next byte expected, how much is on the screen, and how much the backend was told is
  #expected = 0
  #drawn = 0
  #acked = 0
  #acking = false
  #recovering = false
  // an `exit` event that arrived before the output it follows
  #exitAt: ExitEvent | null = null
  // input: what waits to be written, and whether a write is on its way
  #pending: Uint8Array[] = []
  #pendingBytes = 0
  #writing = false

  constructor(rpc: TerminalRpc, events: TerminalEvents) {
    this.#rpc = rpc
    this.#events = events
  }

  get phase(): Phase {
    return this.#phase
  }
  /** How the shell ended, once it has. */
  get exit(): Exit | null {
    return this.#exit
  }
  get failure(): string | null {
    return this.#failure
  }
  get problem(): string | null {
    return this.#problem
  }
  /** The session's id, while there is one (for a test). */
  get session(): string | null {
    return this.#session
  }

  /** Where the output is drawn. Set before `start`. */
  attach(screen: Screen): void {
    this.#screen = screen
  }

  /** Start a session of this size. It never rejects: a failure is kept, to be shown. */
  async start(rows: number, cols: number): Promise<void> {
    this.#size = { rows, cols }
    this.#phase = 'starting'
    this.#failure = null
    this.#problem = null
    this.#exit = null
    this.#exitAt = null
    this.#session = null
    this.#expected = this.#drawn = this.#acked = 0
    this.#pending = []
    this.#pendingBytes = 0
    this.#screen?.reset()
    try {
      const size = this.#size
      const { session } = await this.#rpc.open(size)
      this.#session = session
      this.#told = size
      this.#phase = 'running'
      // the window may have been resized while the shell started
      this.resize(this.#size.rows, this.#size.cols)
    } catch (e) {
      this.#failure = messageOf(e)
      this.#phase = 'failed'
    }
  }

  /** End this session (if the backend still has it) and start another. */
  async restart(): Promise<void> {
    await this.close()
    await this.start(this.#size.rows, this.#size.cols)
  }

  /**
   * Hear the backend while the window is open: the session's output and its end, and the host connection coming
   * back; and say regularly that the page is still there. Returns a function that stops all of it.
   */
  listen(pingMs: number = PING_MS): () => void {
    const stops = [
      this.#events.on('output', (data) => this.#output(data as OutputEvent)),
      this.#events.on('exit', (data) => this.#exited(data as ExitEvent)),
      // events sent while the connection was away reached nobody
      this.#events.on('host:reconnected', () => void this.#recover())
    ]
    const ping = setInterval(() => void this.beat(), pingMs)
    return () => {
      clearInterval(ping)
      stops.forEach((stop) => stop())
    }
  }

  #output(event: OutputEvent): void {
    if (event.session !== this.#session) return // another window's session
    if (event.offset > this.#expected) return void this.#recover() // an event before this one did not arrive
    this.#draw(event.offset, fromBase64(event.data))
    this.#settleExit()
  }

  /** Draw the part of `bytes` (which start at `offset`) that is not on the screen yet. */
  #draw(offset: number, bytes: Uint8Array): void {
    const fresh = bytes.subarray(Math.min(bytes.length, this.#expected - offset))
    if (fresh.length === 0) return
    this.#expected += fresh.length
    const session = this.#session
    const drawn = (): void => {
      if (session !== this.#session) return
      this.#drawn += fresh.length
      if (this.#drawn - this.#acked >= ACK_BYTES) void this.beat()
    }
    if (this.#screen) this.#screen.write(fresh, drawn)
    else drawn()
  }

  #exited(event: ExitEvent): void {
    if (event.session !== this.#session) return
    this.#exitAt = event
    if (event.offset > this.#expected) void this.#recover()
    else this.#settleExit()
  }

  /** The shell has ended and all of its output is here: say so. */
  #settleExit(): void {
    const exit = this.#exitAt
    if (exit === null || exit.offset > this.#expected || this.#phase !== 'running') return
    this.#exit = exit.signal === undefined ? { code: exit.code } : { code: exit.code, signal: exit.signal }
    this.#phase = 'exited'
  }

  /** The backend no longer has the session. */
  #lost(): void {
    if (this.#phase === 'running') this.#phase = 'ended'
    this.#session = null
    this.#pending = []
    this.#pendingBytes = 0
  }

  /** A call failed: the backend refused it (the session is gone), or it never got there (said, and tried again later). */
  #failed(e: unknown, session: string): void {
    if (session !== this.#session) return
    if (refused(e)) this.#lost()
    else this.#problem = messageOf(e)
  }

  /** Ask for the output this page does not have (an event was lost, or the connection was away), one request at a time. */
  async #recover(): Promise<void> {
    const session = this.#session
    if (session === null || this.#recovering) return
    this.#recovering = true
    try {
      const replay = await this.#rpc.replay({ session, from: this.#expected })
      if (session !== this.#session) return
      this.#problem = null
      // older output than the backend keeps is gone: go on from where it starts
      if (replay.from > this.#expected) this.#expected = replay.from
      this.#draw(replay.from, fromBase64(replay.data))
      if (replay.exit) this.#exitAt = { session, offset: this.#expected, ...replay.exit }
      this.#settleExit()
    } catch (e) {
      this.#failed(e, session)
    } finally {
      this.#recovering = false
    }
    // what was typed, and the size the window took, while the connection was away
    void this.#pump()
    void this.#tellSize()
  }

  /**
   * Tell the backend how much is drawn, which also says the page is still there. Its answer says where the output
   * ends: past what this page has, an event was lost.
   */
  async beat(): Promise<void> {
    const session = this.#session
    if (session === null || this.#acking) return
    this.#acking = true
    const offset = this.#drawn
    try {
      const status = await this.#rpc.ack({ session, offset })
      if (session !== this.#session) return
      this.#acked = Math.max(this.#acked, offset)
      this.#problem = null
      if (status.exit) this.#exitAt = { session, offset: status.offset, ...status.exit }
      if (status.offset > this.#expected) void this.#recover()
      else this.#settleExit()
    } catch (e) {
      this.#failed(e, session)
    } finally {
      this.#acking = false
    }
  }

  /** What was typed or pasted, as the terminal emulator gives it (text). */
  type(text: string): void {
    this.typeBytes(encoder.encode(text))
  }

  /** Bytes that are not text (a mouse report in an old encoding). */
  typeBytes(bytes: Uint8Array): void {
    if (this.#phase !== 'running' || bytes.length === 0) return
    if (this.#pendingBytes + bytes.length > PENDING_BYTES) return // the shell is not reading: no more is kept
    this.#pending.push(bytes)
    this.#pendingBytes += bytes.length
    void this.#pump()
  }

  /** Send what waits, one write at a time and in order, each of at most {@link WRITE_BYTES}. */
  async #pump(): Promise<void> {
    if (this.#writing) return
    this.#writing = true
    try {
      while (this.#session !== null && this.#phase === 'running' && this.#pendingBytes > 0) {
        const session = this.#session
        const piece = this.#take()
        try {
          await this.#rpc.write({ session, data: toBase64(piece) })
          this.#problem = null
        } catch (e) {
          // not written: it goes back to the front, to be sent when the connection is back (unless the session is gone)
          if (session === this.#session && !refused(e)) {
            this.#pending.unshift(piece)
            this.#pendingBytes += piece.length
          }
          this.#failed(e, session)
          return
        }
      }
    } finally {
      this.#writing = false
    }
  }

  /** The next piece of input: everything that waits, up to {@link WRITE_BYTES}. */
  #take(): Uint8Array {
    const piece = new Uint8Array(Math.min(this.#pendingBytes, WRITE_BYTES))
    let filled = 0
    while (filled < piece.length) {
      const first = this.#pending[0]!
      const part = first.subarray(0, piece.length - filled)
      piece.set(part, filled)
      filled += part.length
      if (part.length === first.length) this.#pending.shift()
      else this.#pending[0] = first.subarray(part.length)
    }
    this.#pendingBytes -= piece.length
    return piece
  }

  /** The screen now has this many rows and columns: the shell is told the latest size, one call at a time. */
  resize(rows: number, cols: number): void {
    this.#size = { rows, cols }
    void this.#tellSize()
  }

  async #tellSize(): Promise<void> {
    if (this.#resizing) return
    this.#resizing = true
    try {
      while (this.#session !== null && this.#phase === 'running' && (this.#told.rows !== this.#size.rows || this.#told.cols !== this.#size.cols)) {
        const session = this.#session
        const size = this.#size
        try {
          await this.#rpc.resize({ session, ...size })
          this.#told = size
        } catch (e) {
          this.#failed(e, session)
          return
        }
      }
    } finally {
      this.#resizing = false
    }
  }

  /** End the session: the window is closing, or another session takes its place. It never rejects. */
  async close(): Promise<void> {
    const session = this.#session
    this.#session = null
    if (session === null) return
    try {
      await this.#rpc.close({ session })
    } catch {
      // the backend ends a session it does not hear from
    }
  }
}
