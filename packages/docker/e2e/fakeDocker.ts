/**
 * A Docker daemon for the journeys of an app that runs on Docker: the Engine API's paths the container framework
 * calls, answered in Docker's shapes by a server in the test's own process. The journeys point the app at it, so they
 * need no Docker and no image, nothing listens on a port of the machine they run on, no folder has to be there, and
 * what they find is the same on every machine.
 *
 * It starts with the containers and networks it is given (none, for an app that makes its own) beside Docker's own
 * networks (`bridge`, `host`, `none`), keeps the images that were pulled, the volumes that were made and the containers
 * that were made exactly as they were asked for (which is what the journeys look at), changes them as Docker would
 * (refusing what Docker refuses, in its words), and reports each change on `/events`, which is how an app's backend
 * hears of it.
 */
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo } from 'node:net'

/** What a container finds at a place of its own: a folder of the machine (`bind`), or a volume (`source` is its name). */
export interface Mount {
  type: 'bind' | 'volume'
  source: string
  target: string
  readOnly: boolean
}

/** Where a port of a container is published: the machine's address (empty for all) and port (empty for one Docker chooses). */
export interface Binding {
  HostIp: string
  HostPort: string
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
  /** `bridge` when left out. */
  driver?: string
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
  /** `8081/tcp` -> the machine's port (the first it is published on). */
  ports: Record<string, string>
  /** `8081/tcp` -> every binding of it, as it was asked for. */
  bindings: Record<string, Binding[]>
  /** Every port the image exposes is published too. */
  publishAll: boolean
  mounts: Mount[]
  /** The network it is on. */
  network: string
  restart: string
  /** With `on-failure`: how many times at most. */
  retries: number
  workingDir: string
  user: string
  hostname: string
  running: boolean
}

interface Kept extends Container {
  id: string
  /** The ports Docker chose for the bindings that left it to Docker, by the binding's key and place. */
  chosen: Record<string, string>
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
  /** Images the registry does not have: pulling one fails as Docker's pull does. */
  missingImages?: string[]
}

export interface FakeDocker {
  /** Where the app is pointed: `tcp://127.0.0.1:<port>`. */
  readonly host: string
  /** Put the machine back as it starts: its first containers, networks and volumes, only their images, every port free. */
  reset(): void
  /** A container by name; `undefined` when there is none (it was removed, or never made). */
  container(name: string): Container | undefined
  /** How many containers there are. */
  count(): number
  /** The images that were pulled, in order. */
  pulled(): string[]
  /** The names of the networks that are left, Docker's own first. */
  networks(): string[]
  /** The names of the volumes there are, in the order they were made. */
  volumes(): string[]
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
  const missing = new Set(options.missingImages ?? [])
  let made = 0
  const keep = (c: Container, seeded: boolean, log: string[], chosen: Record<string, string> = {}): Kept => {
    made += 1
    return { ...c, id: made.toString(16).padStart(64, '0'), chosen, seeded, exited: false, log }
  }
  /** A whole container from what is known of it: the rest as Docker has it for a container made with nothing more. */
  const container = (c: Partial<Container> & Pick<Container, 'name' | 'image'>): Container => ({
    entrypoint: [],
    cmd: [],
    env: [],
    labels: {},
    ports: {},
    bindings: {},
    publishAll: false,
    mounts: [],
    network: 'bridge',
    restart: 'no',
    retries: 0,
    workingDir: '',
    user: '',
    hostname: '',
    running: false,
    ...c
  })
  const seeds = (): Kept[] =>
    (options.containers ?? []).map((seed) => {
      const stack = seed.labels?.['com.docker.compose.project']
      return keep(
        container({
          name: seed.name,
          image: seed.image,
          entrypoint: ['docker-entrypoint.sh'],
          cmd: ['serve'],
          env: ['PATH=/usr/local/bin:/usr/bin', 'SECRET_TOKEN=do-not-show'],
          labels: { ...seed.labels },
          ports: { ...seed.ports },
          bindings: Object.fromEntries(Object.entries(seed.ports ?? {}).map(([port, host]) => [port, [{ HostIp: '', HostPort: host }]])),
          mounts: [{ type: 'volume', source: `${seed.name}-data`, target: '/data', readOnly: false }],
          network: stack === undefined ? 'bridge' : `${stack}_default`,
          restart: 'unless-stopped',
          running: seed.running
        }),
        true,
        [...(seed.log ?? [])]
      )
    })
  /** Docker's own networks, which every Docker has and none can remove. */
  const OWN: readonly Network[] = [
    { name: 'bridge', driver: 'bridge' },
    { name: 'host', driver: 'host' },
    { name: 'none', driver: 'null' }
  ]
  const firstNetworks = (): Network[] => [...OWN, ...(options.networks ?? [])].map((network) => ({ ...network }))
  interface Volume {
    name: string
    /** Docker named it: it is a container's own, made with it (`docker rm -v` removes it). */
    anonymous: boolean
  }
  const firstVolumes = (kept: Kept[]): Volume[] =>
    kept.flatMap((c) => c.mounts.filter((m) => m.type === 'volume').map((m) => ({ name: m.source, anonymous: false })))

  let containers = seeds()
  let networks = firstNetworks()
  let volumes = firstVolumes(containers)
  // the image of a container that is there is on the machine
  const firstImages = (kept: Kept[]): string[] => [...new Set(kept.map((c) => c.image))]
  let images = firstImages(containers)
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
  // the ports Docker chooses, when a binding leaves it to Docker
  let nextPort = 49153
  /** The machine's port of each binding: the one asked for, or the one Docker chose. */
  const published = (c: Kept): { key: string; ip: string; port: string }[] =>
    Object.entries(c.bindings).flatMap(([key, bindings]) =>
      bindings.map((binding, i) => ({ key, ip: binding.HostIp, port: binding.HostPort === '' ? (c.chosen[`${key}#${i}`] ?? '') : binding.HostPort }))
    )
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
      ? published(c).map(({ key, ip, port }) => ({
          IP: ip === '' ? '0.0.0.0' : ip,
          PrivatePort: Number(key.split('/')[0]),
          PublicPort: Number(port),
          Type: key.split('/')[1] ?? 'tcp'
        }))
      : []
  })

  /** Docker's inspection of a container: what it was made with (`Config`, `HostConfig`), and what it has now. */
  const inspectOf = (c: Kept): object => ({
    Id: c.id,
    Name: `/${c.name}`,
    Created: '2026-11-27T09:00:00.000000000Z',
    Path: c.entrypoint[0] ?? c.cmd[0] ?? '',
    Args: c.entrypoint.length > 0 ? [...c.entrypoint.slice(1), ...c.cmd] : c.cmd.slice(1),
    State: { Status: stateOf(c), Running: c.running, Paused: false, Restarting: false, ExitCode: c.exited ? 137 : 0 },
    Image: `sha256:${'f'.repeat(64)}`,
    Config: {
      // Docker names a container's host after the start of its id
      Hostname: c.hostname === '' ? c.id.slice(0, 12) : c.hostname,
      User: c.user,
      ExposedPorts: Object.fromEntries(Object.keys(c.bindings).map((key) => [key, {}])),
      Env: c.env,
      Cmd: c.cmd,
      Image: c.image,
      WorkingDir: c.workingDir,
      Entrypoint: c.entrypoint.length > 0 ? c.entrypoint : null,
      Labels: c.labels
    },
    HostConfig: {
      NetworkMode: c.network,
      PortBindings: c.bindings,
      RestartPolicy: { Name: c.restart, MaximumRetryCount: c.retries },
      PublishAllPorts: c.publishAll,
      Mounts: c.mounts.map((m) => ({ Type: m.type, Source: m.source, Target: m.target, ReadOnly: m.readOnly }))
    },
    Mounts: c.mounts.map((m) =>
      m.type === 'volume'
        ? { Type: 'volume', Name: m.source, Source: `/var/lib/docker/volumes/${m.source}/_data`, Destination: m.target, Driver: 'local', Mode: 'z', RW: !m.readOnly, Propagation: '' }
        : { Type: 'bind', Source: m.source, Destination: m.target, Mode: '', RW: !m.readOnly, Propagation: 'rprivate' }
    ),
    NetworkSettings: {
      Ports: c.running
        ? Object.fromEntries(Object.keys(c.bindings).map((key) => [key, published(c).filter((p) => p.key === key).map((p) => ({ HostIp: p.ip === '' ? '0.0.0.0' : p.ip, HostPort: p.port }))]))
        : {},
      Networks: { [c.network]: { NetworkID: c.network, EndpointID: c.running ? c.id : '', IPAddress: c.running ? '172.17.0.2' : '' } }
    }
  })

  /** A volume as Docker lists it: the client reads every one of these fields. */
  const volumeOf = (v: Volume): object => ({
    Name: v.name,
    Driver: 'local',
    Mountpoint: `/var/lib/docker/volumes/${v.name}/_data`,
    CreatedAt: '2026-11-27T09:00:00Z',
    Labels: {},
    Scope: 'local',
    Options: {}
  })

  /** A whole path, as Docker takes one for a mount. */
  const absolute = (path: string): boolean => path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path)

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
      const from = url.searchParams.get('fromImage') ?? ''
      const tag = url.searchParams.get('tag')
      const image = `${from}${tag ? `:${tag}` : ''}`
      // the registry has no such image: Docker says so before it pulls anything
      if (missing.has(image)) {
        return json(res, 404, {
          message: `pull access denied for ${from}, repository does not exist or may require 'docker login': denied: requested access to the resource is denied`
        })
      }
      images.push(image)
      pulled.push(image)
      res.writeHead(200, { 'content-type': 'application/json' })
      const step = (status: string, id?: string, progressDetail?: { current: number; total: number }): void =>
        void res.write(`${JSON.stringify({ status, ...(id === undefined ? {} : { id }), ...(progressDetail ? { progressDetail } : {}) })}\n`)
      // two layers, each downloaded in two steps and extracted, as Docker reports a pull
      step(`Pulling from ${from}`, tag ?? 'latest')
      const layers = [
        ['a1b2c3d4e5f6', 3_000_000],
        ['0f9e8d7c6b5a', 1_000_000]
      ] as const
      for (const [id] of layers) step('Pulling fs layer', id)
      for (const [id, total] of layers) {
        step('Downloading', id, { current: total / 2, total })
        step('Downloading', id, { current: total, total })
        step('Download complete', id)
      }
      for (const [id, total] of layers) {
        step('Extracting', id, { current: total, total })
        step('Pull complete', id)
      }
      step(`Digest: sha256:${'e'.repeat(64)}`)
      return void res.end(`${JSON.stringify({ status: `Status: Downloaded newer image for ${image}` })}\n`)
    }
    if (method === 'GET' && path === '/networks') {
      const wanted = filtersOf(url.searchParams).label ?? []
      return json(
        res,
        200,
        networks
          .filter((n) => labelled(n.labels ?? {}, wanted))
          .map((n) => ({ Id: n.name, Name: n.name, Driver: n.driver ?? 'bridge', Scope: 'local', Internal: false, Attachable: false, Labels: n.labels ?? {} }))
      )
    }
    const network = /^\/networks\/([^/]+)$/.exec(path)
    if (method === 'DELETE' && network) {
      if (OWN.some((n) => n.name === network[1])) return json(res, 403, { message: `${network[1]} is a pre-defined network and cannot be removed` })
      networks = networks.filter((n) => n.name !== network[1])
      return void res.writeHead(204).end()
    }
    if (method === 'GET' && path === '/volumes') {
      return json(res, 200, { Volumes: volumes.map(volumeOf), Warnings: [] })
    }
    if (method === 'GET' && path === '/containers/json') {
      const wanted = filtersOf(url.searchParams).label ?? []
      return json(res, 200, containers.filter((c) => labelled(c.labels, wanted)).map(summaryOf))
    }
    if (method === 'POST' && path === '/containers/create') {
      const asked = url.searchParams.get('name')
      if (asked !== null && !/^\/?[a-zA-Z0-9][a-zA-Z0-9_.-]+$/.test(asked)) {
        return json(res, 400, { message: `Invalid container name (${asked}), only [a-zA-Z0-9][a-zA-Z0-9_.-] are allowed` })
      }
      const name = asked?.replace(/^\//, '') ?? `unnamed-${made}`
      const taken = find(name)
      if (taken) {
        return json(res, 409, {
          message: `Conflict. The container name "/${name}" is already in use by container "${taken.id}". You have to remove (or rename) that container to be able to reuse that name.`
        })
      }
      const body = await bodyOf(req)
      const image = String(body.Image ?? '')
      if (!images.includes(image)) return json(res, 404, { message: `No such image: ${image}` })
      const hostConfig = (body.HostConfig ?? {}) as {
        PortBindings?: Record<string, { HostIp?: string; HostPort?: string }[]>
        PublishAllPorts?: boolean
        Mounts?: { Type?: string; Source?: string; Target?: string; ReadOnly?: boolean }[]
        NetworkMode?: string
        RestartPolicy?: { Name?: string; MaximumRetryCount?: number }
      }
      const restart = hostConfig.RestartPolicy?.Name || 'no'
      const retries = hostConfig.RestartPolicy?.MaximumRetryCount ?? 0
      if (retries < 0) return json(res, 400, { message: 'invalid restart policy: maximum retry count cannot be negative' })
      if (retries > 0 && restart !== 'on-failure') {
        return json(res, 400, { message: "invalid restart policy: maximum retry count can only be used with 'on-failure'" })
      }
      const mode = hostConfig.NetworkMode ?? ''
      const network = mode === '' || mode === 'default' ? 'bridge' : mode
      if (!networks.some((n) => n.name === network)) return json(res, 404, { message: `network ${network} not found` })
      const mounts: Mount[] = []
      for (const m of hostConfig.Mounts ?? []) {
        const type = m.Type === 'bind' || m.Type === 'volume' ? m.Type : undefined
        if (type === undefined) return json(res, 400, { message: `mount type unknown: '${m.Type ?? ''}'` })
        const target = m.Target ?? ''
        if (!target.startsWith('/')) {
          return json(res, 400, { message: `invalid mount config for type "${type}": invalid mount path: '${target}' mount path must be absolute` })
        }
        const source = m.Source ?? ''
        if (type === 'bind' && !absolute(source)) {
          return json(res, 400, { message: `invalid mount config for type "bind": invalid mount path: '${source}' mount path must be absolute` })
        }
        mounts.push({ type, source, target, readOnly: m.ReadOnly === true })
      }
      const bindings: Record<string, Binding[]> = Object.fromEntries(
        Object.entries(hostConfig.PortBindings ?? {}).map(([port, list]) => [port, list.map((b) => ({ HostIp: b.HostIp ?? '', HostPort: b.HostPort ?? '' }))])
      )
      // the ports left to Docker are chosen now, and the volumes a container is made with are made with it
      const chosen: Record<string, string> = {}
      for (const [port, list] of Object.entries(bindings)) list.forEach((b, i) => b.HostPort === '' && (chosen[`${port}#${i}`] = String(nextPort++)))
      for (const m of mounts) {
        if (m.type !== 'volume') continue
        if (m.source === '') {
          m.source = (made + volumes.length + 1).toString(16).padStart(64, 'a')
          volumes.push({ name: m.source, anonymous: true })
        } else if (!volumes.some((v) => v.name === m.source)) {
          volumes.push({ name: m.source, anonymous: false })
        }
      }
      const created = keep(
        container({
          name,
          image,
          entrypoint: (body.Entrypoint ?? []) as string[],
          cmd: (body.Cmd ?? []) as string[],
          env: (body.Env ?? []) as string[],
          labels: (body.Labels ?? {}) as Record<string, string>,
          ports: Object.fromEntries(Object.entries(bindings).map(([port, list]) => [port, list[0]?.HostPort || chosen[`${port}#0`] || ''])),
          bindings,
          publishAll: hostConfig.PublishAllPorts === true,
          mounts,
          network,
          restart,
          retries,
          workingDir: String(body.WorkingDir ?? ''),
          user: String(body.User ?? ''),
          hostname: String(body.Hostname ?? '')
        }),
        false,
        [],
        chosen
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
      const yes = (flag: string): boolean => ['1', 'true'].includes(url.searchParams.get(flag) ?? '')
      if (found.running && !yes('force')) {
        return json(res, 409, {
          message: `cannot remove container "/${found.name}": container is running: stop the container before removing or force remove`
        })
      }
      containers = containers.filter((c) => c !== found)
      // its anonymous volumes go with it when asked; a named volume is never a container's own
      if (yes('v')) {
        const own = new Set(found.mounts.filter((m) => m.type === 'volume').map((m) => m.source))
        volumes = volumes.filter((v) => !(v.anonymous && own.has(v.name)))
      }
      tell('destroy', found)
      return void res.writeHead(204).end()
    }
    if (method === 'POST' && (verb === 'start' || verb === 'restart')) {
      const mine = published(found)
      const port = mine[0]?.port
      if (portTaken && port !== undefined) {
        return json(res, 500, {
          message: `driver failed programming external connectivity on endpoint ${found.name}: Bind for 0.0.0.0:${port} failed: port is already allocated`
        })
      }
      // a port of the machine is one running container's
      const clash = mine.find((p) =>
        containers.some((c) => c !== found && c.running && published(c).some((q) => q.port === p.port && q.key.split('/')[1] === p.key.split('/')[1] && (q.ip === p.ip || q.ip === '' || p.ip === '')))
      )
      if (clash) {
        return json(res, 500, {
          message: `driver failed programming external connectivity on endpoint ${found.name}: Bind for ${clash.ip === '' ? '0.0.0.0' : clash.ip}:${clash.port} failed: port is already allocated`
        })
      }
      found.running = true
      found.exited = false
      started.push(found.name)
      found.log.push(...startLog)
      tell('start', found)
      return void res.writeHead(204).end()
    }
    if (method === 'POST' && verb === 'rename') {
      const asked = (url.searchParams.get('name') ?? '').replace(/^\//, '')
      if (!/^[a-zA-Z0-9][a-zA-Z0-9_.-]+$/.test(asked)) {
        return json(res, 400, { message: `Invalid container name (${asked}), only [a-zA-Z0-9][a-zA-Z0-9_.-] are allowed` })
      }
      const taken = find(asked)
      if (taken && taken !== found) {
        return json(res, 409, { message: `Conflict. The container name "/${asked}" is already in use by container "${taken.id}". You have to remove (or rename) that container to be able to reuse that name.` })
      }
      found.name = asked
      tell('rename', found)
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
      volumes = firstVolumes(containers)
      images = firstImages(containers)
      nextPort = 49153
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
          bindings: c.bindings,
          publishAll: c.publishAll,
          mounts: c.mounts,
          network: c.network,
          restart: c.restart,
          retries: c.retries,
          workingDir: c.workingDir,
          user: c.user,
          hostname: c.hostname,
          running: c.running
        }
      )
    },
    count: () => containers.length,
    pulled: () => [...pulled],
    networks: () => networks.map((n) => n.name),
    volumes: () => volumes.map((v) => v.name),
    started: () => [...started],
    takePort(taken) {
      portTaken = taken
    },
    run(name, image) {
      const ran = keep(container({ name, image, running: true }), false, ['hello'])
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
