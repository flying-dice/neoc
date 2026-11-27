/**
 * A fake ACP agent for the Agents journeys: the part of the Agent Client Protocol (v1) the app speaks, over a
 * WebSocket as `acpd` serves one (one JSON-RPC message per text frame, `initialize` first, one agent per socket). The
 * WebSocket is the few lines of RFC 6455 a test needs, on `node:http`, so the spec needs no package for it.
 *
 * What the person says picks what the agent does (`SCRIPTS` in journeys.spec.ts). Its sessions outlive a socket, as
 * an agent that keeps them on disk does: a new connection can load one. `?load=no` in the URL makes an agent that
 * cannot load a session.
 */
import { createHash } from 'node:crypto'
import { createServer, type IncomingMessage, type Server } from 'node:http'
import type { AddressInfo } from 'node:net'
import type { Duplex } from 'node:stream'

/** A JSON-RPC message, as the wire has it (camelCase). */
export type Message = { jsonrpc: '2.0'; id?: number | string; method?: string; params?: any; result?: any; error?: { code: number; message: string } }

/** One socket's WebSocket: text frames in and out. */
class Socket {
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
  /** The socket it is on now. */
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

/** One socket: an agent process, as acpd runs one per socket. */
export class Connection {
  readonly socket: Socket
  readonly headers: IncomingMessage['headers']
  readonly load: boolean
  #next = 1
  readonly #waiting = new Map<number | string, (message: Message) => void>()
  readonly #cancelled = new Set<string>()
  readonly #wakers = new Set<() => void>()

  constructor(
    readonly agent: FakeAgent,
    stream: Duplex,
    request: IncomingMessage
  ) {
    this.headers = request.headers
    this.load = new URL(request.url ?? '/', 'http://x').searchParams.get('load') !== 'no'
    this.socket = new Socket(
      stream,
      (text) => void this.#receive(JSON.parse(text) as Message),
      () => {
        agent.connections.delete(this)
        for (const wake of this.#wakers) wake()
      }
    )
  }

  get closed(): boolean {
    return this.socket.closed
  }

  #answer(id: Message['id'], result: unknown): void {
    this.socket.send({ jsonrpc: '2.0', id, result })
  }

  #fail(id: Message['id'], code: number, message: string): void {
    this.socket.send({ jsonrpc: '2.0', id, error: { code, message } })
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
    this.socket.send({ jsonrpc: '2.0', method: 'session/update', params: { sessionId, update } })
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
          this.socket.send({ jsonrpc: '2.0', id: asked, method: 'session/request_permission', params: { sessionId: session.id, toolCall, options } })
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

/** The agent: a server on a loopback port the system picks. */
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
      this.connections.add(new Connection(this, stream, request))
    })
  }

  /** Listen, and return the agent's address (`ws://127.0.0.1:<port>/acp`). */
  async start(): Promise<string> {
    await new Promise<void>((resolve) => this.#server.listen(0, '127.0.0.1', resolve))
    return `ws://127.0.0.1:${(this.#server.address() as AddressInfo).port}/acp`
  }

  async stop(): Promise<void> {
    for (const connection of this.connections) connection.socket.close()
    await new Promise<void>((resolve) => this.#server.close(() => resolve()))
  }

  /** The session the last prompt that said `text` went to. */
  sessionOf(text: string): Session | undefined {
    return [...this.sessions.values()].findLast((s) => s.said.includes(text))
  }
}
