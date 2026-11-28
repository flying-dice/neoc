/**
 * A fake ACP agent for the journeys of an app that talks to one: the part of the Agent Client Protocol (v1) the apps
 * speak, on either of the two connections they make. The Agents app reaches it over a WebSocket as `acpd` serves one
 * (one JSON-RPC message per text frame, `initialize` first, one agent per socket); the WebSocket is the few lines of
 * RFC 6455 a test needs, on `node:http`, so the specs need no package for it. A container agent is a container's
 * program, spoken to on the container's standard streams, a JSON-RPC message a line (`serve`, given the fake Docker's
 * attached client); a program agent is e2e/acp-relay.ts, which relays its standard streams to the WebSocket. Both are
 * one agent: the same sessions, scripts and record of what it was sent.
 *
 * What the person says picks what the agent does (the scripts a spec gives it). Its sessions outlive a connection, as
 * an agent that keeps them on disk does: a new connection can load one. `?load=no` in the URL makes an agent that
 * cannot load a session, and `?initialize=never` one that takes the connection and never answers `initialize`.
 */
import { createHash } from 'node:crypto'
import { createServer, type IncomingHttpHeaders, type IncomingMessage, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import type { Duplex } from 'node:stream'

/** A JSON-RPC message, as the wire has it (camelCase). */
export type Message = { jsonrpc: '2.0'; id?: number | string; method?: string; params?: any; result?: any; error?: { code: number; message: string } }

/** How a connection reaches the client: a message out, and the end of it. Messages in reach the connection's `onText`. */
interface Wire {
  readonly closed: boolean
  send(message: Message): void
  close(): void
}

/** A program's standard streams, from the program's side: what the fake Docker's attached client is. */
export interface Lines {
  /** The next bytes written to its input; `null` once the input is closed. */
  read(): Promise<Uint8Array | null>
  /** Write to its output. */
  write(data: string): void
  /** End, as the program exiting does. */
  end(): void
}

/** A program that speaks a JSON-RPC message a line on its standard streams; it exits when its input is closed. */
class LineWire implements Wire {
  #closed = false
  constructor(
    readonly lines: Lines,
    onText: (text: string) => void,
    readonly onClose: () => void
  ) {
    void (async () => {
      const decoder = new TextDecoder()
      let pending = ''
      for (;;) {
        const chunk = await lines.read()
        if (chunk === null) break
        pending += decoder.decode(chunk, { stream: true })
        for (let cut = pending.indexOf('\n'); cut !== -1; cut = pending.indexOf('\n')) {
          const line = pending.slice(0, cut).trim()
          pending = pending.slice(cut + 1)
          if (line !== '') onText(line)
        }
      }
      this.close()
    })()
  }

  get closed(): boolean {
    return this.#closed
  }

  send(message: Message): void {
    if (!this.#closed) this.lines.write(`${JSON.stringify(message)}\n`)
  }

  close(): void {
    if (this.#closed) return
    this.#closed = true
    this.lines.end()
    this.onClose()
  }
}

/** One socket's WebSocket: text frames in and out. */
class Socket implements Wire {
  #buffer = Buffer.alloc(0)
  #pieces: Buffer[] = []
  #closed = false
  constructor(
    readonly stream: Duplex,
    readonly onText: (text: string) => void,
    readonly onClose: () => void
  ) {
    stream.on('data', (chunk: Buffer) => this.#read(chunk))
    stream.on('close', () => this.#end())
    // the other end went away without a close frame (its process stopped): this end goes too
    stream.on('end', () => {
      stream.end()
      this.#end()
    })
    stream.on('error', () => this.#end())
  }

  get closed(): boolean {
    return this.#closed
  }

  #end(): void {
    if (this.#closed) return
    this.#closed = true
    this.onClose()
  }

  #read(chunk: Buffer): void {
    this.#buffer = Buffer.concat([this.#buffer, chunk])
    for (;;) {
      const b = this.#buffer
      if (b.length < 2) return
      const fin = (b[0]! & 0x80) !== 0
      const opcode = b[0]! & 0x0f
      const masked = (b[1]! & 0x80) !== 0
      let length = b[1]! & 0x7f
      let at = 2
      if (length === 126) {
        if (b.length < 4) return
        length = b.readUInt16BE(2)
        at = 4
      } else if (length === 127) {
        if (b.length < 10) return
        length = Number(b.readBigUInt64BE(2))
        at = 10
      }
      const mask = masked ? b.subarray(at, at + 4) : undefined
      if (masked) at += 4
      if (b.length < at + length) return
      const payload = Buffer.from(b.subarray(at, at + length))
      if (mask) for (let i = 0; i < payload.length; i++) payload[i]! ^= mask[i % 4]!
      this.#buffer = b.subarray(at + length)
      if (opcode === 0x8) {
        this.#frame(0x8, Buffer.alloc(0))
        this.stream.end()
        this.#end()
        return
      }
      if (opcode === 0x9) this.#frame(0xa, payload)
      if (opcode === 0x1 || opcode === 0x0) {
        this.#pieces.push(payload)
        if (fin) {
          const text = Buffer.concat(this.#pieces).toString('utf8')
          this.#pieces = []
          this.onText(text)
        }
      }
    }
  }

  #frame(opcode: number, payload: Buffer): void {
    if (this.stream.destroyed) return
    const length = payload.length
    let head: Buffer
    if (length < 126) head = Buffer.from([0x80 | opcode, length])
    else if (length < 65536) head = Buffer.from([0x80 | opcode, 126, length >> 8, length & 0xff])
    else {
      head = Buffer.alloc(10)
      head.writeUInt8(0x80 | opcode, 0)
      head.writeUInt8(127, 1)
      head.writeBigUInt64BE(BigInt(length), 2)
    }
    this.stream.write(Buffer.concat([head, payload]))
  }

  send(message: Message): void {
    if (!this.#closed) this.#frame(0x1, Buffer.from(JSON.stringify(message), 'utf8'))
  }

  close(): void {
    this.#frame(0x8, Buffer.alloc(0))
    this.stream.end()
    this.#end()
  }
}

/** A session of the agent's: what was said in it, and the MCP servers it was given. */
export interface Session {
  id: string
  cwd: string
  mcpServers: any[]
  said: string[]
  /** The connection it is on now. */
  connection: Connection
}

/** What a script does in a turn: send updates, ask for permissions, and say how the turn ends. */
export interface Turn {
  readonly session: Session
  readonly text: string
  /** Send a `session/update`. */
  update(update: Record<string, any>): void
  /** A piece of the answer. */
  say(text: string): void
  /** Ask for a permission; resolves with the outcome (`{ outcome: 'selected', optionId }` or `{ outcome: 'cancelled' }`). */
  ask(toolCall: Record<string, any>, options: { optionId: string; name: string; kind: string }[]): Promise<{ outcome: string; optionId?: string }>
  /** Whether the client cancelled the turn (`session/cancel`). */
  readonly cancelled: boolean
  /** Wait, unless the turn is cancelled first. */
  pause(ms: number): Promise<void>
}

/** A script: what the agent does with a prompt. Returns the stop reason, or throws `{ code, message }` for an error. */
export type Script = (turn: Turn) => Promise<string | void>

/** One connection: an agent process, as acpd runs one per socket and a container runs one as its program. */
export class Connection {
  readonly wire: Wire
  #next = 1
  readonly #waiting = new Map<number | string, (message: Message) => void>()
  readonly #cancelled = new Set<string>()
  readonly #wakers = new Set<() => void>()

  constructor(
    readonly agent: FakeAgent,
    /** The connection's wire, given what to do with a message that comes and with its end. */
    wire: (onText: (text: string) => void, onClose: () => void) => Wire,
    /** The headers of the WebSocket's upgrade; none on standard streams. */
    readonly headers: IncomingHttpHeaders = {},
    /** Whether it can load a session. */
    readonly load = true,
    /** Whether it answers `initialize` at all. */
    readonly initializes = true
  ) {
    agent.connections.add(this)
    this.wire = wire(
      (text) => void this.#receive(JSON.parse(text) as Message),
      () => {
        agent.connections.delete(this)
        for (const wake of this.#wakers) wake()
      }
    )
  }

  get closed(): boolean {
    return this.wire.closed
  }

  #answer(id: Message['id'], result: unknown): void {
    this.wire.send({ jsonrpc: '2.0', id, result })
  }

  #fail(id: Message['id'], code: number, message: string): void {
    this.wire.send({ jsonrpc: '2.0', id, error: { code, message } })
  }

  async #receive(message: Message): Promise<void> {
    this.agent.received.push(message)
    if (message.method === undefined) {
      // an answer to a request of ours
      this.#waiting.get(message.id!)?.(message)
      this.#waiting.delete(message.id!)
      return
    }
    const { id, method, params } = message
    if (method === 'initialize') {
      if (!this.initializes) return
      return this.#answer(id, {
        protocolVersion: 1,
        agentCapabilities: { loadSession: this.load, promptCapabilities: {}, mcpCapabilities: { http: true, sse: false } },
        agentInfo: { name: 'fake-agent', version: '1.0.0' },
        authMethods: []
      })
    }
    if (method === 'session/new') {
      const session: Session = { id: `session-${this.agent.sessions.size + 1}`, cwd: params.cwd, mcpServers: params.mcpServers, said: [], connection: this }
      this.agent.sessions.set(session.id, session)
      return this.#answer(id, { sessionId: session.id })
    }
    if (method === 'session/load') {
      const session = this.agent.sessions.get(params.sessionId)
      if (!this.load) return this.#fail(id, -32601, 'Method not found')
      if (!session) return this.#fail(id, -32002, 'Resource not found')
      Object.assign(session, { cwd: params.cwd, mcpServers: params.mcpServers, connection: this })
      // the history, replayed before the answer: the client drops it
      for (const said of session.said) {
        this.#notify(session.id, { sessionUpdate: 'user_message_chunk', content: { type: 'text', text: said } })
      }
      return this.#answer(id, {})
    }
    if (method === 'session/cancel') {
      this.#cancelled.add(params.sessionId)
      for (const wake of this.#wakers) wake()
      return
    }
    if (method === 'session/prompt') return this.#prompt(message)
    if (id !== undefined) this.#fail(id, -32601, 'Method not found')
  }

  #notify(sessionId: string, update: Record<string, any>): void {
    this.wire.send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId, update } })
  }

  async #prompt({ id, params }: Message): Promise<void> {
    const session = this.agent.sessions.get(params.sessionId)
    if (!session || session.connection !== this) return this.#fail(id, -32002, 'Resource not found')
    const text = (params.prompt as { type: string; text?: string }[]).map((b) => b.text ?? '').join('')
    session.said.push(text)
    this.#cancelled.delete(session.id)
    const self = this
    const turn: Turn = {
      session,
      text,
      update: (update) => this.#notify(session.id, update),
      say: (piece) => this.#notify(session.id, { sessionUpdate: 'agent_message_chunk', content: { type: 'text', text: piece } }),
      ask: (toolCall, options) =>
        new Promise((resolve) => {
          const asked = `permission-${this.#next++}`
          this.#waiting.set(asked, (answer) => resolve(answer.result?.outcome ?? { outcome: 'cancelled' }))
          this.wire.send({ jsonrpc: '2.0', id: asked, method: 'session/request_permission', params: { sessionId: session.id, toolCall, options } })
        }),
      get cancelled() {
        return self.#cancelled.has(session.id) || self.closed
      },
      pause: (ms) =>
        new Promise<void>((resolve) => {
          const done = (): void => {
            clearTimeout(timer)
            this.#wakers.delete(done)
            resolve()
          }
          const timer = setTimeout(done, ms)
          this.#wakers.add(done)
        })
    }
    const script = this.agent.scripts.find(([match]) => match(text))?.[1] ?? (async (t: Turn) => t.say(`echo: ${t.text}`))
    try {
      const stop = await script(turn)
      this.#answer(id, { stopReason: turn.cancelled ? 'cancelled' : (stop ?? 'end_turn') })
    } catch (e) {
      const { code = -32603, message = String(e) } = e as { code?: number; message?: string }
      this.#fail(id, code, message)
    }
  }
}

/** The agent: a server on a loopback port the system picks (`start`), and a program on standard streams (`serve`). */
export class FakeAgent {
  readonly #server: Server
  readonly connections = new Set<Connection>()
  readonly sessions = new Map<string, Session>()
  /** Every message a client sent, in order. */
  readonly received: Message[] = []

  constructor(readonly scripts: [match: (text: string) => boolean, script: Script][]) {
    this.#server = createServer((_, res) => res.writeHead(404).end())
    this.#server.on('upgrade', (request: IncomingMessage, stream: Duplex) => {
      const key = request.headers['sec-websocket-key']
      if (!request.url?.startsWith('/acp') || typeof key !== 'string') {
        stream.end('HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\n\r\n')
        return
      }
      const accept = createHash('sha1').update(`${key}258EAFA5-E914-47DA-95CA-C5AB0DC85B11`).digest('base64')
      stream.write(`HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: ${accept}\r\n\r\n`)
      const query = new URL(request.url ?? '/', 'http://x').searchParams
      const load = query.get('load') !== 'no'
      const initializes = query.get('initialize') !== 'never'
      new Connection(this, (onText, onClose) => new Socket(stream, onText, onClose), request.headers, load, initializes)
    })
  }

  /** Be the program on these standard streams (a container's, attached), a JSON-RPC message a line, until its input closes. */
  serve(lines: Lines): Connection {
    return new Connection(this, (onText, onClose) => new LineWire(lines, onText, onClose))
  }

  /** Listen, and return the agent's address (`ws://127.0.0.1:<port>/acp`). */
  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.#server.listen(0, '127.0.0.1', resolve))
    return `ws://127.0.0.1:${(this.#server.address() as AddressInfo).port}/acp`
  }

  async stop(): Promise<void> {
    for (const connection of this.connections) connection.wire.close()
    if (this.#server.listening) await new Promise<void>((resolve) => this.#server.close(() => resolve()))
  }

  /** The session the last prompt that said `text` went to. */
  sessionOf(text: string): Session | undefined {
    return [...this.sessions.values()].findLast((s) => s.said.includes(text))
  }
}
