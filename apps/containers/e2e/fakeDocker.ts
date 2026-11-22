/**
 * A Docker daemon for the journeys: the Engine API's paths the Containers app calls, answered in Docker's shapes by a
 * server in the test's own process. The journeys point the app at it, so they need no Docker, and what they find is
 * the same on every machine. It keeps the containers of one small machine (a Compose stack that is up, one that is
 * down, a container on its own), changes them as Docker would, and reports each change on `/events`, which is how the
 * app's backend hears of it.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'

interface FakeContainer {
  id: string
  name: string
  image: string
  running: boolean
  /** The Compose stack and service it belongs to; none for a container on its own. */
  stack?: string
  service?: string
  /** The service it waits for. */
  needs?: string
  /** `[the machine's port, the container's]`. */
  port?: [number, number]
  log: string[]
}

export interface FakeDocker {
  /** Where the app is pointed: `tcp://127.0.0.1:<port>`. */
  readonly host: string
  /** Put the machine back as it starts. */
  reset(): void
  /** Whether a container runs; `undefined` once it is removed. */
  running(name: string): boolean | undefined
  /** The names of the networks that are left. */
  networks(): string[]
  /** The order the containers were started in since the last reset, by name. */
  started(): string[]
  /** Docker changes behind the app, as a terminal would change it: a container is run. */
  run(name: string, image: string): void
  /** A container writes a line. */
  write(name: string, line: string): void
  close(): Promise<void>
}

const machine = (): FakeContainer[] => [
  { id: 'a1'.repeat(32), name: 'blog-db-1', image: 'postgres:16', running: true, stack: 'blog', service: 'db', log: ['database system is ready'] },
  {
    id: 'b2'.repeat(32),
    name: 'blog-web-1',
    image: 'ghost:5',
    running: true,
    stack: 'blog',
    service: 'web',
    needs: 'db',
    port: [8080, 2368],
    log: ['Ghost is running', 'listening on 2368']
  },
  { id: 'c3'.repeat(32), name: 'media-jellyfin-1', image: 'jellyfin/jellyfin:10.9', running: false, stack: 'media', service: 'jellyfin', log: [] },
  { id: 'd4'.repeat(32), name: 'pihole', image: 'pihole/pihole:2024.07.0', running: true, port: [8053, 80], log: ['FTL started'] }
]

const labelsOf = (c: FakeContainer): Record<string, string> =>
  c.stack === undefined
    ? {}
    : {
        'com.docker.compose.project': c.stack,
        'com.docker.compose.service': c.service ?? '',
        'com.docker.compose.project.working_dir': `/srv/${c.stack}`,
        ...(c.needs === undefined ? {} : { 'com.docker.compose.depends_on': `${c.needs}:service_started:false` })
      }

const summaryOf = (c: FakeContainer): object => ({
  Id: c.id,
  Names: [`/${c.name}`],
  Image: c.image,
  State: c.running ? 'running' : 'exited',
  Status: c.running ? 'Up 2 hours' : 'Exited (0) 3 days ago',
  Labels: labelsOf(c),
  Ports: c.port && c.running ? [{ IP: '0.0.0.0', PrivatePort: c.port[1], PublicPort: c.port[0], Type: 'tcp' }] : []
})

const inspectOf = (c: FakeContainer): object => ({
  Id: c.id,
  Name: `/${c.name}`,
  Path: 'docker-entrypoint.sh',
  Args: ['serve'],
  State: { Status: c.running ? 'running' : 'exited', Running: c.running },
  Config: { Image: c.image, Env: ['PATH=/usr/local/bin:/usr/bin', 'SECRET_TOKEN=do-not-show'], Labels: labelsOf(c) },
  HostConfig: { RestartPolicy: { Name: 'unless-stopped' } },
  Mounts: [{ Type: 'volume', Name: `${c.name}-data`, Destination: '/data' }],
  NetworkSettings: { Networks: { [c.stack === undefined ? 'bridge' : `${c.stack}_default`]: {} } }
})

/** One frame of Docker's multiplexed output: the stream (1 is stdout), three zero bytes, the length, the bytes. */
function frame(line: string): Buffer {
  const data = Buffer.from(`${line}\n`)
  const head = Buffer.alloc(8)
  head[0] = 1
  head.writeUInt32BE(data.length, 4)
  return Buffer.concat([head, data])
}

export async function startFakeDocker(): Promise<FakeDocker> {
  let containers = machine()
  let networks = ['blog_default', 'media_default']
  let started: string[] = []
  // the app's backend, reading `/events`
  const listeners = new Set<ServerResponse>()

  const tell = (action: string, c: FakeContainer): void => {
    const event = JSON.stringify({ Type: 'container', Action: action, Actor: { ID: c.id, Attributes: { name: c.name } }, time: 1 })
    for (const listener of listeners) listener.write(`${event}\n`)
  }
  const find = (key: string): FakeContainer | undefined => containers.find((c) => c.id === key || c.name === key)
  /** The stack a `filters` query asks for, when it asks for one. */
  const stackOf = (query: URLSearchParams): string | undefined => {
    const filters = JSON.parse(query.get('filters') ?? '{}') as { label?: string[] }
    return filters.label?.map((l) => /^com\.docker\.compose\.project=(.*)$/.exec(l)?.[1]).find((s) => s !== undefined)
  }

  const json = (res: ServerResponse, status: number, body: unknown): void => {
    res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
  }
  const missing = (res: ServerResponse, key: string): void => json(res, 404, { message: `No such container: ${key}` })

  function answer(req: IncomingMessage, res: ServerResponse): void {
    const url = new URL(req.url ?? '/', 'http://docker')
    // a client may name the API version first (`/v1.47/containers/json`)
    const path = url.pathname.replace(/^\/v\d+\.\d+/, '')
    const method = req.method ?? 'GET'

    if (path === '/version') return json(res, 200, { Version: '27.3.1', ApiVersion: '1.47', Os: 'linux', Arch: 'arm64' })
    if (path === '/_ping') return void res.writeHead(200, { 'content-type': 'text/plain' }).end('OK')
    if (path === '/events') {
      res.writeHead(200, { 'content-type': 'application/json' })
      res.flushHeaders()
      listeners.add(res)
      req.on('close', () => listeners.delete(res))
      return
    }
    if (method === 'GET' && path === '/containers/json') {
      const stack = stackOf(url.searchParams)
      return json(res, 200, containers.filter((c) => stack === undefined || c.stack === stack).map(summaryOf))
    }
    if (method === 'GET' && path === '/networks') {
      const stack = stackOf(url.searchParams)
      return json(res, 200, networks.filter((n) => stack === undefined || n === `${stack}_default`).map((n) => ({ Id: n, Name: n })))
    }
    const network = /^\/networks\/([^/]+)$/.exec(path)
    if (method === 'DELETE' && network) {
      networks = networks.filter((n) => n !== network[1])
      return void res.writeHead(204).end()
    }

    const [, key, verb] = /^\/containers\/([^/]+)(?:\/(\w+))?$/.exec(path) ?? []
    if (key === undefined) return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
    const found = find(key)
    if (!found) return missing(res, key)

    if (method === 'GET' && verb === 'json') return json(res, 200, inspectOf(found))
    if (method === 'GET' && verb === 'logs') {
      const tail = Number(url.searchParams.get('tail') ?? 'all')
      const lines = Number.isNaN(tail) ? found.log : found.log.slice(-tail)
      return void res.writeHead(200, { 'content-type': 'application/vnd.docker.multiplexed-stream' }).end(Buffer.concat(lines.map(frame)))
    }
    if (method === 'DELETE' && verb === undefined) {
      containers = containers.filter((c) => c !== found)
      tell('destroy', found)
      return void res.writeHead(204).end()
    }
    if (method === 'POST' && (verb === 'start' || verb === 'stop' || verb === 'restart')) {
      found.running = verb !== 'stop'
      if (found.running) started.push(found.name)
      found.log.push(verb === 'stop' ? 'shutting down' : 'started again')
      tell(verb === 'stop' ? 'die' : 'start', found)
      return void res.writeHead(204).end()
    }
    return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
  }

  const server = createServer(answer)
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo

  return {
    host: `tcp://127.0.0.1:${port}`,
    reset() {
      containers = machine()
      networks = ['blog_default', 'media_default']
      started = []
    },
    running: (name) => find(name)?.running,
    networks: () => [...networks],
    started: () => [...started],
    run(name, image) {
      const made: FakeContainer = { id: Buffer.from(name).toString('hex').padEnd(64, '0'), name, image, running: true, log: ['hello'] }
      containers.push(made)
      tell('start', made)
    },
    write(name, line) {
      find(name)?.log.push(line)
    },
    close: () =>
      new Promise((resolve) => {
        for (const listener of listeners) listener.end()
        server.closeAllConnections()
        server.close(() => resolve())
      })
  }
}
