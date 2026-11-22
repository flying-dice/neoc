import { describe, expect, test } from 'bun:test'
import { createFakeBackend, type FakeBackend } from './fakeApi'
import { ACK_BYTES, Terminal, WRITE_BYTES, fromBase64, toBase64, type Screen } from './terminal.svelte'

/** A screen that keeps what was drawn as text. */
class Recorder implements Screen {
  text = ''
  resets = 0
  readonly #decoder = new TextDecoder()
  write(bytes: Uint8Array, drawn: () => void): void {
    this.text += this.#decoder.decode(bytes, { stream: true })
    drawn()
  }
  reset(): void {
    this.text = ''
    this.resets += 1
  }
}

/** Let the answers that are due arrive (the fake answers on a later turn of the event loop). */
async function settle(): Promise<void> {
  for (let i = 0; i < 12; i++) await new Promise((resolve) => setTimeout(resolve, 0))
}

interface Opened {
  backend: FakeBackend
  terminal: Terminal
  screen: Recorder
  stop: () => void
}

async function opened(backend: FakeBackend = createFakeBackend()): Promise<Opened> {
  const terminal = new Terminal(backend.rpc, backend.events)
  const screen = new Recorder()
  terminal.attach(screen)
  // an hour between pings: a test says when the page beats
  const stop = terminal.listen(3_600_000)
  await terminal.start(24, 80)
  await settle()
  return { backend, terminal, screen, stop }
}

describe('a session', () => {
  test('starts a shell of the screen size and shows its prompt', async () => {
    const { backend, terminal, screen, stop } = await opened()
    expect(terminal.phase).toBe('running')
    expect(backend.sizes).toEqual([{ rows: 24, cols: 80 }])
    expect(screen.text).toBe('$ ')
    stop()
  })

  test('sends what is typed and draws what the shell answers', async () => {
    const { backend, terminal, screen, stop } = await opened()
    terminal.type('echo hi\r')
    await settle()
    expect(backend.written.join('')).toBe('echo hi\r')
    expect(screen.text).toBe('$ echo hi\r\nhi\r\n$ ')
    stop()
  })

  test('sends one write at a time, in order, and what was typed meanwhile goes in the next', async () => {
    const { backend, terminal, stop } = await opened()
    for (const key of 'echo abc') terminal.type(key)
    terminal.type('\r')
    await settle()
    expect(backend.written.join('')).toBe('echo abc\r')
    // the first key left alone; the rest was typed while it was on its way
    expect(backend.written[0]).toBe('e')
    expect(backend.written.length).toBeLessThan(9)
    stop()
  })

  test('a paste longer than one write goes in pieces, whole and in order', async () => {
    const { backend, terminal, stop } = await opened()
    const paste = 'x'.repeat(WRITE_BYTES * 2 + 10)
    terminal.type(paste)
    await settle()
    expect(backend.written.map((w) => w.length)).toEqual([WRITE_BYTES, WRITE_BYTES, 10])
    expect(backend.written.join('')).toBe(paste)
    stop()
  })

  test('tells the shell the latest size, also one given while it was starting', async () => {
    const backend = createFakeBackend()
    const terminal = new Terminal(backend.rpc, backend.events)
    const starting = terminal.start(24, 80)
    terminal.resize(30, 100)
    await starting
    await settle()
    terminal.resize(40, 120)
    terminal.resize(41, 121)
    await settle()
    expect(backend.sizes[0]).toEqual({ rows: 24, cols: 80 })
    expect(backend.sizes.at(-1)).toEqual({ rows: 41, cols: 121 })
    expect(backend.sizes).toContainEqual({ rows: 30, cols: 100 })
  })

  test('ignores the events of a session that is not its own', async () => {
    const { backend, screen, stop } = await opened()
    backend.emit('output', { session: 'someone-else', offset: 0, data: toBase64(new TextEncoder().encode('not mine')) })
    backend.emit('exit', { session: 'someone-else', offset: 0, code: 0 })
    await settle()
    expect(screen.text).toBe('$ ')
    stop()
  })
})

describe('the end of a shell', () => {
  test('exit shows how it ended, and nothing more is sent', async () => {
    const { backend, terminal, stop } = await opened()
    terminal.type('exit 3\r')
    await settle()
    expect(terminal.phase).toBe('exited')
    expect(terminal.exit).toEqual({ code: 3 })
    const before = backend.written.length
    terminal.type('ls\r')
    await settle()
    expect(backend.written.length).toBe(before)
    stop()
  })

  test('a new session closes the old one and starts on an empty screen', async () => {
    const { backend, terminal, screen, stop } = await opened()
    const first = terminal.session
    terminal.type('exit\r')
    await settle()
    await terminal.restart()
    await settle()
    expect(terminal.phase).toBe('running')
    expect(terminal.session).not.toBe(first)
    expect(backend.open()).toEqual([terminal.session!])
    expect(screen.text).toBe('$ ')
    stop()
  })

  test('closing ends the session on the backend', async () => {
    const { backend, terminal, stop } = await opened()
    await terminal.close()
    expect(backend.open()).toEqual([])
    stop()
  })

  test('a shell that cannot be started is a failure with the reason', async () => {
    const backend = createFakeBackend({ refuse: 'The shell could not be started.' })
    const terminal = new Terminal(backend.rpc, backend.events)
    await terminal.start(24, 80)
    expect(terminal.phase).toBe('failed')
    expect(terminal.failure).toBe('The shell could not be started.')
  })
})

describe('output that did not arrive', () => {
  test('an event lost in the middle is asked for again, and the output is whole and in order', async () => {
    const { backend, terminal, screen, stop } = await opened()
    const session = terminal.session!
    backend.lose(1)
    backend.print(session, 'one ')
    backend.print(session, 'two ')
    await settle()
    expect(screen.text).toBe('$ one two ')
    backend.print(session, 'three')
    await settle()
    expect(screen.text).toBe('$ one two three')
    stop()
  })

  test('a lost last event is found by the next beat', async () => {
    const { backend, terminal, screen, stop } = await opened()
    backend.lose(1)
    backend.print(terminal.session!, 'quiet')
    await settle()
    expect(screen.text).toBe('$ ')
    await terminal.beat()
    await settle()
    expect(screen.text).toBe('$ quiet')
    stop()
  })

  test('a lost exit is found by the next beat', async () => {
    const { backend, terminal, stop } = await opened()
    backend.lose(8)
    terminal.type('exit 2\r')
    await settle()
    expect(terminal.phase).toBe('running')
    backend.lose(0)
    await terminal.beat()
    await settle()
    expect(terminal.phase).toBe('exited')
    expect(terminal.exit).toEqual({ code: 2 })
    stop()
  })

  test('the page says what it has drawn once enough is unacknowledged, and on every beat', async () => {
    const { backend, terminal, stop } = await opened()
    await terminal.beat()
    expect(backend.acks).toEqual([2])
    backend.print(terminal.session!, 'x'.repeat(ACK_BYTES))
    await settle()
    expect(backend.acks.at(-1)).toBe(2 + ACK_BYTES)
    stop()
  })
})

describe('the host connection', () => {
  test('while it is away the reason is shown and what is typed waits; when it is back the session goes on', async () => {
    const { backend, terminal, screen, stop } = await opened()
    backend.offline(true)
    terminal.type('echo back\r')
    await settle()
    expect(terminal.phase).toBe('running')
    expect(terminal.problem).toBe('The host is not answering.')
    // the shell printed meanwhile, to nobody
    backend.lose(1)
    backend.print(terminal.session!, 'missed ')
    backend.offline(false)
    backend.emit('host:reconnected', null)
    await settle()
    expect(terminal.problem).toBeNull()
    expect(screen.text).toBe('$ missed echo back\r\nback\r\n$ ')
    stop()
  })

  test('a session the backend ended while the page was away is said to have ended', async () => {
    const { backend, terminal, stop } = await opened()
    backend.forget()
    backend.emit('host:reconnected', null)
    await settle()
    expect(terminal.phase).toBe('ended')
    expect(terminal.session).toBeNull()
    await terminal.restart()
    await settle()
    expect(terminal.phase).toBe('running')
    stop()
  })
})

describe('bytes', () => {
  test('base64 round-trips every byte', () => {
    const bytes = new Uint8Array(70_000).map((_, i) => i % 256)
    expect(fromBase64(toBase64(bytes))).toEqual(bytes)
  })
})
