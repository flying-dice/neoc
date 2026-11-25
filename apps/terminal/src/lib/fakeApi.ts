/**
 * A stand-in for the Terminal app's backend, for stories and unit tests: sessions kept in the page, with no host and no
 * process. Its "shell" is a few lines of script: it prints a `$ ` prompt, echoes what is typed as a terminal does, and
 * knows `echo <text>`, `stty size`, `exit [code]` and nothing else. Like the backend it numbers its output, keeps it for
 * `replay`, and can be told to lose an event, to stop answering, or to forget a session.
 */
import type { Exit } from '@terminal/rpc'
import { fromBase64, toBase64, type TerminalEvents, type TerminalRpc } from './terminal.svelte'

export interface FakeBackend {
  readonly rpc: TerminalRpc
  readonly events: TerminalEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string, data?: unknown): void
  /** What the page typed, decoded, one entry per `write`. */
  readonly written: string[]
  /** Every `ack` the page sent: how much it said it had drawn. */
  readonly acks: number[]
  /** Every size the page told a session. */
  readonly sizes: { rows: number; cols: number }[]
  /** The sessions that are open, by id. */
  open(): string[]
  /** The next `n` events are sent to nobody, as the host does for a client that is behind. */
  lose(n: number): void
  /** Stop answering (every call fails as a call that never arrived does), or answer again. */
  offline(away: boolean): void
  /** Forget every session, as the backend does after the page was away for too long. */
  forget(): void
  /** Print `text` in a session as the shell's own output (a program that prints by itself). */
  print(session: string, text: string): void
}

export interface FakeOptions {
  /** `open` fails with this message: no shell can be started. */
  refuse?: string
  /** The prompt the shell prints. */
  prompt?: string
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {
  readonly code = -32010
}

interface Session {
  rows: number
  cols: number
  line: string
  /** Everything the shell printed, for `replay`. */
  output: Uint8Array
  exit?: Exit
}

const encoder = new TextEncoder()
const decoder = new TextDecoder()

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const prompt = options.prompt ?? '$ '
  const sessions = new Map<string, Session>()
  const listeners = new Map<string, Set<(data: unknown) => void>>()
  const written: string[] = []
  const acks: number[] = []
  const sizes: { rows: number; cols: number }[] = []
  let next = 1
  let losing = 0
  let away = false

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))
  const deliver = (name: string, data: unknown): void => {
    if (losing > 0) losing -= 1
    else emit(name, data)
  }
  const answer = async (): Promise<void> => {
    await new Promise((resolve) => setTimeout(resolve, 0))
    if (away) throw new Error('The host is not answering.')
  }
  const find = (id: string): Session => {
    const session = sessions.get(id)
    if (!session) throw new Refused('This session has ended.')
    return session
  }

  function print(id: string, text: string): void {
    const session = sessions.get(id)
    if (!session || text === '') return
    const bytes = encoder.encode(text)
    const offset = session.output.length
    const output = new Uint8Array(offset + bytes.length)
    output.set(session.output)
    output.set(bytes, offset)
    session.output = output
    deliver('output', { session: id, offset, data: toBase64(bytes) })
  }

  function run(id: string, session: Session, line: string): void {
    const command = line.trim()
    if (command === 'exit' || command.startsWith('exit ')) {
      const code = Number(command.slice(5)) || 0
      session.exit = { code }
      return deliver('exit', { session: id, offset: session.output.length, code })
    }
    if (command.startsWith('echo ')) print(id, `${command.slice(5)}\r\n`)
    else if (command === 'stty size') print(id, `${session.rows} ${session.cols}\r\n`)
    else if (command !== '') print(id, `sh: ${command.split(' ')[0]}: command not found\r\n`)
    print(id, prompt)
  }

  function typed(id: string, session: Session, text: string): void {
    for (const char of text) {
      if (session.exit) return
      if (char === '\r' || char === '\n') {
        const line = session.line
        session.line = ''
        print(id, '\r\n')
        run(id, session, line)
      } else if (char === '\u007f') {
        if (session.line !== '') {
          session.line = session.line.slice(0, -1)
          print(id, '\b \b')
        }
      } else {
        session.line += char
        print(id, char)
      }
    }
  }

  const rpc: TerminalRpc = {
    open: async ({ rows, cols }) => {
      await answer()
      if (options.refuse !== undefined) throw new Refused(options.refuse)
      const id = `session-${next++}`
      sessions.set(id, { rows, cols, line: '', output: new Uint8Array() })
      sizes.push({ rows, cols })
      // the prompt follows the answer, as the shell's first output follows `open`
      setTimeout(() => print(id, prompt), 0)
      return { session: id }
    },
    write: async ({ session, data }) => {
      await answer()
      const found = find(session)
      if (found.exit) throw new Refused('The shell has exited.')
      const text = decoder.decode(fromBase64(data))
      written.push(text)
      typed(session, found, text)
      return null
    },
    resize: async ({ session, rows, cols }) => {
      await answer()
      const found = find(session)
      found.rows = rows
      found.cols = cols
      sizes.push({ rows, cols })
      return null
    },
    ack: async ({ session, offset }) => {
      await answer()
      const found = find(session)
      acks.push(offset)
      return found.exit ? { offset: found.output.length, exit: found.exit } : { offset: found.output.length }
    },
    replay: async ({ session, from }) => {
      await answer()
      const found = find(session)
      const start = Math.min(from, found.output.length)
      const data = toBase64(found.output.subarray(start))
      return found.exit ? { from: start, data, exit: found.exit } : { from: start, data }
    },
    close: async ({ session }) => {
      await answer()
      sessions.delete(session)
      return null
    }
  }

  const events: TerminalEvents = {
    on(name, fn) {
      const set = listeners.get(name) ?? new Set()
      listeners.set(name, set)
      set.add(fn)
      return () => set.delete(fn)
    }
  }

  return {
    rpc,
    events,
    emit,
    written,
    acks,
    sizes,
    open: () => [...sessions.keys()],
    lose: (n) => void (losing = n),
    offline: (value) => void (away = value),
    forget: () => sessions.clear(),
    print
  }
}
