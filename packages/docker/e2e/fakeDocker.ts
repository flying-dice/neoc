/**
 * A Docker daemon for the journeys of an app that runs on Docker: the Engine API's paths the container framework
 * calls, answered in Docker's shapes by a server in the test's own process. The journeys point the app at it, so they
 * need no Docker and no image, nothing listens on a port of the machine they run on, no folder has to be there, and
 * what they find is the same on every machine.
 *
 * It starts with the containers and networks it is given (none, for an app that makes its own), keeps the images that
 * were pulled and the containers that were made exactly as they were asked for (which is what the journeys look at),
 * changes them as Docker would, and reports each change on `/events`, which is how an app's backend hears of it.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'

/** A folder of the machine, as a container is given it. */
export interface Mount {
  source: string
  target: string
  readOnly: boolean
}

/** A container the machine starts with. */
export interface Seed {
  name: string
  image: string
  running: boolean
  labels?: Record<string, string>
  /** `2368/tcp` -> the machine's port. */
  ports?: Record<string, string>
  /** What it has written so far. */
  log?: string[]
}

/** A network the machine starts with. */
export interface Network {
  name: string
  labels?: Record<string, string>
}

/** A container, as the journeys read it: what it was made from, and whether it runs. */
export interface Container {
  name: string
  image: string
  entrypoint: string[]
  cmd: string[]
  /** `NAME=value`, each. */
  env: string[]
  labels: Record<string, string>
  /** `8081/tcp` -> the machine's port. */
  ports: Record<string, string>
  mounts: Mount[]
  restart: string
  running: boolean
}

interface Kept extends Container {
  id: string
  /** It was there when the machine started: it has been up, or down, for a while. */
  seeded: boolean
  /** It ran, and stopped. */
  exited: boolean
  log: string[]
}

export interface Options {
  /** The containers the machine starts with, and goes back to on `reset`. */
  containers?: Seed[]
  /** The networks it starts with. */
  networks?: Network[]
  /** What a container writes when it starts. */
  startLog?: string[]
  /** Where a container writes: its output (the default) or its errors. An app shows both together. */
  logStream?: 'stdout' | 'stderr'
}

export interface FakeDocker {
  /** Where the app is pointed: `tcp://127.0.0.1:<port>`. */
  readonly host: string
  /** Put the machine back as it starts: its first containers and networks, no image, every port free. */
  reset(): void
  /** A container by name; `undefined` when there is none (it was removed, or never made). */
  container(name: string): Container | undefined
  /** How many containers there are. */
  count(): number
  /** The images that were pulled, in order. */
  pulled(): string[]
  /** The names of the networks that are left. */
  networks(): string[]
  /** The order the containers were started in since the last reset, by name. */
  started(): string[]
  /** Something else has a container's ports (or lets go of them): Docker refuses to start a container that publishes one. */
  takePort(taken: boolean): void
  /** Docker changes behind the app, as a terminal would change it: a container is run. */
  run(name: string, image: string): void
  /** A container stops behind the app, as a crash or a terminal would stop it. */
  stop(name: string): void
  /** A container writes a line. */
  write(name: string, line: string): void
  close(): Promise<void>
}

async function bodyOf(req: IncomingMessage): Promise<Record<string, unknown>> {
  const chunks: Buffer[] = []
  for await (const chunk of req) chunks.push(chunk as Buffer)
  const text = Buffer.concat(chunks).toString('utf8')
  return text === '' ? {} : (JSON.parse(text) as Record<string, unknown>)
}

export async function startFakeDocker(options: Options = {}): Promise<FakeDocker> {
  const stream = options.logStream === 'stderr' ? 2 : 1
  const startLog = options.startLog ?? ['started']
  let made = 0
  const keep = (c: Container, seeded: boolean, log: string[]): Kept => {
    made += 1
    return { ...c, id: made.toString(16).padStart(64, '0'), seeded, exited: false, log }
  }
  const seeds = (): Kept[] =>
    (options.containers ?? []).map((seed) =>
      keep(
        {
          name: seed.name,
          image: seed.image,
          entrypoint: ['docker-entrypoint.sh'],
          cmd: ['serve'],
          env: ['PATH=/usr/local/bin:/usr/bin', 'SECRET_TOKEN=do-not-show'],
          labels: { ...seed.labels },
          ports: { ...seed.ports },
          mounts: [],
          restart: 'unless-stopped',
          running: seed.running
        },
        true,
        [...(seed.log ?? [])]
      )
    )
  const firstNetworks = (): Network[] => (options.networks ?? []).map((network) => ({ ...network }))

  let containers = seeds()
  let networks = firstNetworks()
  let images: string[] = []
  let pulled: string[] = []
  let started: string[] = []
  let portTaken = false
  // the app's backend, reading `/events`
  const listeners = new Set<ServerResponse>()

  const tell = (action: string, c: Kept): void => {
    const event = JSON.stringify({ Type: 'container', Action: action, Actor: { ID: c.id, Attributes: { name: c.name, ...c.labels } }, time: 1 })
    for (const listener of listeners) listener.write(`${event}\n`)
  }
  const find = (key: string): Kept | undefined => containers.find((c) => c.id === key || c.name === key)
  const filtersOf = (query: URLSearchParams): { label?: string[]; reference?: string[] } =>
    JSON.parse(query.get('filters') ?? '{}') as { label?: string[]; reference?: string[] }
  /** Docker's `label` filter: `key`, or `key=value`, every one of them. */
  const labelled = (labels: Record<string, string>, wanted: string[]): boolean =>
    wanted.every((label) => {
      const at = label.indexOf('=')
      const key = at === -1 ? label : label.slice(0, at)
      return key in labels && (at === -1 || labels[key] === label.slice(at + 1))
    })

  const json = (res: ServerResponse, status: number, body: unknown): void => {
    res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
  }

  /** One frame of Docker's multiplexed output: the stream (1 is its output, 2 its errors), three zero bytes, the length, the bytes. */
  const frame = (line: string): Buffer => {
    const data = Buffer.from(`${line}\n`)
    const head = Buffer.alloc(8)
    head[0] = stream
    head.writeUInt32BE(data.length, 4)
    return Buffer.concat([head, data])
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

  const stateOf = (c: Kept): string => (c.running ? 'running' : c.exited || c.seeded ? 'exited' : 'created')
  const statusOf = (c: Kept): string => {
    if (c.running) return c.seeded ? 'Up 2 hours' : 'Up 1 second'
    if (c.seeded && !c.exited) return 'Exited (0) 3 days ago'
    return c.exited ? 'Exited (137) 1 second ago' : 'Created'
  }

  const summaryOf = (c: Kept): object => ({
    Id: c.id,
    Names: [`/${c.name}`],
    Image: c.image,
    State: stateOf(c),
    Status: statusOf(c),
    Labels: c.labels,
    // a container that does not run has no port of the machine
    Ports: c.running
      ? Object.entries(c.ports).map(([port, host]) => ({ IP: '0.0.0.0', PrivatePort: Number(port.split('/')[0]), PublicPort: Number(host), Type: port.split('/')[1] ?? 'tcp' }))
      : []
  })

  const inspectOf = (c: Kept): object => ({
    Id: c.id,
    Name: `/${c.name}`,
    Path: c.entrypoint[0] ?? '',
    Args: [...c.entrypoint.slice(1), ...c.cmd],
    State: { Status: stateOf(c), Running: c.running },
    Config: { Image: c.image, Env: c.env, Labels: c.labels },
    HostConfig: { RestartPolicy: { Name: c.restart } },
    Mounts: [
      { Type: 'volume', Name: `${c.name}-data`, Destination: '/data' },
      ...c.mounts.map((m) => ({ Type: 'bind', Source: m.source, Destination: m.target, RW: !m.readOnly }))
    ],
    NetworkSettings: {
      Networks: { [c.labels['com.docker.compose.project'] === undefined ? 'bridge' : `${c.labels['com.docker.compose.project']}_default`]: {} }
    }
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
    if (method === 'GET' && path === '/networks') {
      const wanted = filtersOf(url.searchParams).label ?? []
      return json(res, 200, networks.filter((n) => labelled(n.labels ?? {}, wanted)).map((n) => ({ Id: n.name, Name: n.name, Labels: n.labels ?? {} })))
    }
    const network = /^\/networks\/([^/]+)$/.exec(path)
    if (method === 'DELETE' && network) {
      networks = networks.filter((n) => n.name !== network[1])
      return void res.writeHead(204).end()
    }
    if (method === 'GET' && path === '/containers/json') {
      const wanted = filtersOf(url.searchParams).label ?? []
      return json(res, 200, containers.filter((c) => labelled(c.labels, wanted)).map(summaryOf))
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
      const created = keep(
        {
          name,
          image,
          entrypoint: (body.Entrypoint ?? []) as string[],
          cmd: (body.Cmd ?? []) as string[],
          env: (body.Env ?? []) as string[],
          labels: (body.Labels ?? {}) as Record<string, string>,
          ports: Object.fromEntries(Object.entries(hostConfig.PortBindings ?? {}).map(([port, bindings]) => [port, bindings[0]?.HostPort ?? ''])),
          mounts: (hostConfig.Mounts ?? []).map((m) => ({ source: m.Source ?? '', target: m.Target ?? '', readOnly: m.ReadOnly === true })),
          restart: hostConfig.RestartPolicy?.Name ?? 'no',
          running: false
        },
        false,
        []
      )
      containers.push(created)
      tell('create', created)
      return json(res, 201, { Id: created.id, Warnings: [] })
    }

    const [, key, verb] = /^\/containers\/([^/]+)(?:\/(\w+))?$/.exec(path) ?? []
    if (key === undefined) return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
    const found = find(key)
    if (!found) return json(res, 404, { message: `No such container: ${key}` })

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
    if (method === 'POST' && (verb === 'start' || verb === 'restart')) {
      const port = Object.values(found.ports)[0]
      if (portTaken && port !== undefined) {
        return json(res, 500, {
          message: `driver failed programming external connectivity on endpoint ${found.name}: Bind for 0.0.0.0:${port} failed: port is already allocated`
        })
      }
      found.running = true
      found.exited = false
      started.push(found.name)
      found.log.push(...startLog)
      tell('start', found)
      return void res.writeHead(204).end()
    }
    if (method === 'POST' && verb === 'stop') {
      found.running = false
      found.exited = true
      found.log.push('shutting down')
      tell('die', found)
      return void res.writeHead(204).end()
    }
    return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
  }

  const server = createServer((req, res) => {
    answer(req, res).catch((e: unknown) => json(res, 500, { message: String(e) }))
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo

  return {
    host: `tcp://127.0.0.1:${port}`,
    reset() {
      containers = seeds()
      networks = firstNetworks()
      images = []
      pulled = []
      started = []
      portTaken = false
    },
    container(name) {
      const c = find(name)
      return (
        c && {
          name: c.name,
          image: c.image,
          entrypoint: c.entrypoint,
          cmd: c.cmd,
          env: c.env,
          labels: c.labels,
          ports: c.ports,
          mounts: c.mounts,
          restart: c.restart,
          running: c.running
        }
      )
    },
    count: () => containers.length,
    pulled: () => [...pulled],
    networks: () => networks.map((n) => n.name),
    started: () => [...started],
    takePort(taken) {
      portTaken = taken
    },
    run(name, image) {
      const ran = keep(
        { name, image, entrypoint: [], cmd: [], env: [], labels: {}, ports: {}, mounts: [], restart: 'no', running: true },
        false,
        ['hello']
      )
      containers.push(ran)
      tell('start', ran)
    },
    stop(name) {
      const c = find(name)
      if (!c) return
      c.running = false
      c.exited = true
      tell('die', c)
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
