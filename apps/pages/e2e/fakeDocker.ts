/**
 * A Docker daemon for the journeys: the Engine API's paths the Pages app calls, answered in Docker's shapes by a
 * server in the test's own process. The journeys point the app at it, so they need no Docker and no image, nothing
 * listens on a port of the machine they run on, and no folder has to be there. It keeps the images that were pulled
 * and the containers that were made, exactly as they were asked for (which is what the journeys look at: the server
 * is its container's configuration, ports and folders), and reports each change on `/events`, which is how the app's
 * backend hears of it.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'

interface Made {
  id: string
  name: string
  image: string
  entrypoint: string[]
  env: string[]
  labels: Record<string, string>
  /** `8081/tcp` -> the machine's port. */
  ports: Record<string, string>
  mounts: Mount[]
  restart: string
  running: boolean
  /** It ran, and stopped. */
  exited: boolean
  log: string[]
}

/** A folder of the machine, as a container is given it. */
export interface Mount {
  source: string
  target: string
  readOnly: boolean
}

/** The server's container, as the journeys read it. */
export interface Sidecar {
  name: string
  image: string
  entrypoint: string[]
  /** nginx's configuration, as the container's environment carries it. */
  config: string
  /** `8081/tcp` -> the machine's port. */
  ports: Record<string, string>
  mounts: Mount[]
  restart: string
  running: boolean
}

export interface FakeDocker {
  /** Where the app is pointed: `tcp://127.0.0.1:<port>`. */
  readonly host: string
  /** Put the machine back as it starts: no image, no container, the port free. */
  reset(): void
  /** The app's container; `undefined` when there is none. */
  sidecar(): Sidecar | undefined
  /** How many containers there are. */
  count(): number
  /** The images that were pulled, in order. */
  pulled(): string[]
  /** Something else has one of the ports (or lets go of it): Docker refuses to start a container that publishes it. */
  takePort(taken: boolean): void
  /** The container stops behind the app, as a crash or a terminal would stop it. */
  stop(): void
  /** The server writes a line. */
  write(line: string): void
  close(): Promise<void>
}

const PORT_TAKEN =
  'driver failed programming external connectivity on endpoint vos-pages: Bind for 0.0.0.0:8081 failed: port is already allocated'

/** One frame of Docker's multiplexed output: the stream (1 is stdout, where nginx logs what it answered), three zero bytes, the length, the bytes. */
function frame(line: string): Buffer {
  const data = Buffer.from(`${line}\n`)
  const head = Buffer.alloc(8)
  head[0] = 1
  head.writeUInt32BE(data.length, 4)
  return Buffer.concat([head, data])
}

async function bodyOf(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = []
  for await (const chunk of req) chunks.push(chunk as Buffer)
  const text = Buffer.concat(chunks).toString('utf8')
  return text === '' ? {} : (JSON.parse(text) as Record<string, unknown>)
}

export async function startFakeDocker(): Promise<FakeDocker> {
  let containers: Made[] = []
  let images: string[] = []
  let pulled: string[] = []
  let portTaken = false
  let made = 0
  // the app's backend, reading `/events`
  const listeners = new Set<ServerResponse>()

  const tell = (action: string, c: Made): void => {
    const event = JSON.stringify({ Type: 'container', Action: action, Actor: { ID: c.id, Attributes: { name: c.name, ...c.labels } }, time: 1 })
    for (const listener of listeners) listener.write(`${event}\n`)
  }
  const find = (key: string): Made | undefined => containers.find((c) => c.id === key || c.name === key)
  const filtersOf = (query: URLSearchParams): { label?: string[]; reference?: string[] } =>
    JSON.parse(query.get('filters') ?? '{}') as { label?: string[]; reference?: string[] }
  const labelled = (c: Made, wanted: string[]): boolean =>
    wanted.every((label) => {
      const [key, value] = label.split('=') as [string, string | undefined]
      return key in c.labels && (value === undefined || c.labels[key] === value)
    })

  const json = (res: ServerResponse, status: number, body: unknown): void => {
    res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
  }

  /** An image as Docker lists it: the client reads every one of these fields. */
  const imageOf = (image: string): object => ({
    Id: `sha256:${'f'.repeat(64)}`,
    ParentId: '',
    RepoTags: [image],
    RepoDigests: [],
    Created: 1_700_000_000,
    Size: 9_000_000,
    SharedSize: -1,
    Labels: {},
    Containers: -1
  })

  const summaryOf = (c: Made): object => ({
    Id: c.id,
    Names: [`/${c.name}`],
    Image: c.image,
    State: c.running ? 'running' : c.exited ? 'exited' : 'created',
    Status: c.running ? 'Up 1 second' : c.exited ? 'Exited (137) 1 second ago' : 'Created',
    Labels: c.labels,
    Ports: []
  })

  async function answer(req: IncomingMessage, res: ServerResponse): Promise<void> {
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
    if (method === 'GET' && path === '/images/json') {
      const wanted = filtersOf(url.searchParams).reference ?? []
      return json(res, 200, images.filter((image) => wanted.length === 0 || wanted.includes(image)).map(imageOf))
    }
    if (method === 'POST' && path === '/images/create') {
      const tag = url.searchParams.get('tag')
      const image = `${url.searchParams.get('fromImage') ?? ''}${tag ? `:${tag}` : ''}`
      images.push(image)
      pulled.push(image)
      res.writeHead(200, { 'content-type': 'application/json' })
      res.write(`${JSON.stringify({ status: `Pulling from ${image}` })}\n`)
      return void res.end(`${JSON.stringify({ status: `Status: Downloaded newer image for ${image}` })}\n`)
    }
    if (method === 'GET' && path === '/containers/json') {
      const wanted = filtersOf(url.searchParams).label ?? []
      return json(res, 200, containers.filter((c) => labelled(c, wanted)).map(summaryOf))
    }
    if (method === 'POST' && path === '/containers/create') {
      const name = url.searchParams.get('name') ?? `unnamed-${made}`
      if (find(name)) return json(res, 409, { message: `Conflict. The container name "/${name}" is already in use.` })
      const body = await bodyOf(req)
      const image = String(body.Image ?? '')
      if (!images.includes(image)) return json(res, 404, { message: `No such image: ${image}` })
      const hostConfig = (body.HostConfig ?? {}) as {
        PortBindings?: Record<string, { HostPort?: string }[]>
        Mounts?: { Type?: string; Source?: string; Target?: string; ReadOnly?: boolean }[]
        RestartPolicy?: { Name?: string }
      }
      made += 1
      const created: Made = {
        id: made.toString(16).padStart(64, '0'),
        name,
        image,
        entrypoint: (body.Entrypoint ?? []) as string[],
        env: (body.Env ?? []) as string[],
        labels: (body.Labels ?? {}) as Record<string, string>,
        ports: Object.fromEntries(Object.entries(hostConfig.PortBindings ?? {}).map(([port, bindings]) => [port, bindings[0]?.HostPort ?? ''])),
        mounts: (hostConfig.Mounts ?? []).map((m) => ({ source: m.Source ?? '', target: m.Target ?? '', readOnly: m.ReadOnly === true })),
        restart: hostConfig.RestartPolicy?.Name ?? 'no',
        running: false,
        exited: false,
        log: []
      }
      containers.push(created)
      tell('create', created)
      return json(res, 201, { Id: created.id, Warnings: [] })
    }

    const [, key, verb] = /^\/containers\/([^/]+)(?:\/(\w+))?$/.exec(path) ?? []
    if (key === undefined) return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
    const found = find(key)
    if (!found) return json(res, 404, { message: `No such container: ${key}` })

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
    if (method === 'POST' && verb === 'start') {
      if (portTaken) return json(res, 500, { message: PORT_TAKEN })
      found.running = true
      found.log.push('nginx/1.30.0', 'start worker processes')
      tell('start', found)
      return void res.writeHead(204).end()
    }
    return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
  }

  const server = createServer((req, res) => {
    answer(req, res).catch((e: unknown) => json(res, 500, { message: String(e) }))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo
  const first = (): Made | undefined => containers[0]

  return {
    host: `tcp://127.0.0.1:${port}`,
    reset() {
      containers = []
      images = []
      pulled = []
      portTaken = false
    },
    sidecar() {
      const c = first()
      return (
        c && {
          name: c.name,
          image: c.image,
          entrypoint: c.entrypoint,
          config: (c.env.find((variable) => variable.startsWith('VOS_NGINX_CONF=')) ?? '').slice('VOS_NGINX_CONF='.length),
          ports: c.ports,
          mounts: c.mounts,
          restart: c.restart,
          running: c.running
        }
      )
    },
    count: () => containers.length,
    pulled: () => [...pulled],
    takePort(taken) {
      portTaken = taken
    },
    stop() {
      const c = first()
      if (!c) return
      c.running = false
      c.exited = true
      tell('die', c)
    },
    write(line) {
      first()?.log.push(line)
    },
    close: () =>
      new Promise((resolve) => {
        for (const listener of listeners) listener.end()
        server.closeAllConnections()
        server.close(() => resolve())
      })
  }
}
