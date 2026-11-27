/**
 * A Docker daemon for the journeys of an app that runs on Docker: the Engine API's paths the container framework
 * calls, answered in Docker's shapes by a server in the test's own process. The journeys point the app at it, so they
 * need no Docker and no image, nothing listens on a port of the machine they run on, no folder has to be there, and
 * what they find is the same on every machine.
 *
 * It starts with the containers, networks and images it is given (none, for an app that makes its own) beside Docker's
 * own networks (`bridge`, `host`, `none`), keeps the images that were pulled, the volumes and networks that were made
 * and the containers that were made exactly as they were asked for (which is what the journeys look at), changes them
 * as Docker would (refusing what Docker refuses, in its words: an image a container was made from, a volume a
 * container uses, a network a running container is on), and reports each change on `/events`, which is how an app's
 * backend hears of it. It builds images (any context makes the image the build names), takes archives uploaded into a
 * container, and lets a journey stand in for the program a container runs: what a client attached to a container
 * writes to its input, the journey reads, and what the journey writes is the container's output.
 */
import { createHash } from 'node:crypto'
import { createServer, type IncomingMessage, type ServerResponse } from 'node:http'
import type { AddressInfo, Socket } from 'node:net'

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

/** An image the machine starts with, beside those of its containers: one without tags is a dangling one. */
export interface ImageSeed {
  tags: string[]
  /** In bytes. */
  size?: number
}

/** An image, as the journeys read it. */
export interface Image {
  id: string
  tags: string[]
  size: number
}

/** A volume, as the journeys read it. */
export interface Volume {
  name: string
  driver: string
  labels: Record<string, string>
}

/** A network, as the journeys read it: what it was made with. */
export interface NetworkMade {
  name: string
  driver: string
  subnet: string
  gateway: string
  internal: boolean
  attachable: boolean
  labels: Record<string, string>
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
  /** The id of the image it was made from. */
  imageId: string
  /** The ports Docker chose for the bindings that left it to Docker, by the binding's key and place. */
  chosen: Record<string, string>
  /** It was there when the machine started: it has been up, or down, for a while. */
  seeded: boolean
  /** It ran, and stopped. */
  exited: boolean
  log: string[]
  /** It has a TTY: what it writes is one raw stream, not frames. */
  tty: boolean
}

/** An archive uploaded into a container (`PUT /containers/{id}/archive`). */
export interface Upload {
  /** The directory it was unpacked into. */
  path: string
  /** The tar archive, as it was sent. */
  archive: Uint8Array
}

/**
 * A client attached to a container's streams (`POST /containers/{id}/attach`), from the container's side: the
 * journey is the program the container runs.
 */
export interface Attached {
  /** The container's name. */
  readonly name: string
  /** The next bytes the client wrote to the container's input, as they came; `null` once the client closed it. */
  read(): Promise<Uint8Array | null>
  /** Write to the container's output (or its errors), as one frame of Docker's multiplexed stream (raw with a TTY). */
  write(data: string | Uint8Array, stream?: 'stdout' | 'stderr'): void
  /** End the connection, as the container's program exiting does. */
  end(): void
}

export interface Options {
  /** The containers the machine starts with, and goes back to on `reset`. */
  containers?: Seed[]
  /** The networks it starts with. */
  networks?: Network[]
  /** The images it starts with, beside those of its containers. */
  images?: ImageSeed[]
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
  /** A network by name, as it was made; `undefined` when there is none. */
  network(name: string): NetworkMade | undefined
  /** The names of the volumes there are, in the order they were made. */
  volumes(): string[]
  /** A volume by name; `undefined` when there is none. */
  volume(name: string): Volume | undefined
  /** The images there are, in the order they came. */
  images(): Image[]
  /** The archives uploaded into a container since the last reset, in order. */
  uploads(name: string): Upload[]
  /** The next client to attach to a container (one that is attached already, first). */
  attached(name: string): Promise<Attached>
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

async function bytesOf(req: IncomingMessage): Promise<Buffer> {
  const chunks: Buffer[] = []
  for await (const chunk of req) chunks.push(chunk as Buffer)
  return Buffer.concat(chunks)
}

async function bodyOf(req: IncomingMessage): Promise<Record<string, unknown>> {
  const text = (await bytesOf(req)).toString('utf8')
  return text === '' ? {} : (JSON.parse(text) as Record<string, unknown>)
}


/** Docker's hex digest of a text: an id that is the same on every machine for the same thing. */
const digest = (text: string): string => createHash('sha256').update(text).digest('hex')
/** An id as Docker shortens it in a message: its first twelve hex digits. */
const short = (id: string): string => id.replace(/^sha256:/, '').slice(0, 12)

/** Docker's networks as a 32-bit number, and back: `172.18.0.0` is 2886860800. */
const ipNumber = (ip: string): number => ip.split('.').reduce((n, part) => n * 256 + Number(part), 0)
/** The first and last address of an IPv4 subnet (`172.18.0.0/16`); `undefined` for one that is not IPv4. */
function range(subnet: string): [number, number] | undefined {
  const m = /^(\d+\.\d+\.\d+\.\d+)\/(\d+)$/.exec(subnet)
  if (!m) return undefined
  const size = 2 ** (32 - Number(m[2]))
  const first = Math.floor(ipNumber(m[1]!) / size) * size
  return [first, first + size - 1]
}

export async function startFakeDocker(options: Options = {}): Promise<FakeDocker> {
  const stream = options.logStream === 'stderr' ? 2 : 1
  const startLog = options.startLog ?? ['started']
  const missing = new Set(options.missingImages ?? [])

  interface KeptImage extends Image {
    /** When it was built, in seconds since 1970. */
    created: number
  }
  /** An image by its tags: its id and size follow from them, the same on every machine. */
  const imageOf = (tags: string[], size?: number, salt = ''): KeptImage => {
    const hash = digest(`${tags.join(',')}${salt}`)
    return {
      id: `sha256:${hash}`,
      tags,
      size: size ?? 20_000_000 + (parseInt(hash.slice(0, 6), 16) % 400) * 1_000_000,
      created: 1_700_000_000 + (parseInt(hash.slice(6, 12), 16) % 10_000_000)
    }
  }
  // the images of the containers it starts with are there, then those it is given
  const firstImages = (): KeptImage[] => {
    const kept: KeptImage[] = []
    for (const seed of options.containers ?? []) if (!kept.some((i) => i.tags.includes(seed.image))) kept.push(imageOf([seed.image]))
    ;(options.images ?? []).forEach((seed, i) => kept.push(imageOf([...seed.tags], seed.size, seed.tags.length === 0 ? `#${i}` : '')))
    return kept
  }
  let images = firstImages()
  /** An image by a name (`nginx` is `nginx:latest`) or by its id, whole or the start of it. */
  const imageBy = (key: string): { image: KeptImage; tag?: string } | undefined => {
    const tag = /:[^/]*$/.test(key) || key.includes('@') ? key : `${key}:latest`
    const tagged = images.find((i) => i.tags.includes(tag))
    if (tagged) return { image: tagged, tag }
    const hex = key.replace(/^sha256:/, '')
    if (!/^[0-9a-f]+$/.test(hex)) return undefined
    const byId = images.find((i) => i.id.replace(/^sha256:/, '').startsWith(hex))
    return byId && { image: byId }
  }

  let made = 0
  const keep = (c: Container, seeded: boolean, log: string[], chosen: Record<string, string> = {}): Kept => {
    made += 1
    const image = imageBy(c.image)?.image.id ?? `sha256:${digest(c.image)}`
    return { ...c, id: made.toString(16).padStart(64, '0'), imageId: image, chosen, seeded, exited: false, log, tty: false }
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

  interface KeptNetwork extends NetworkMade {
    id: string
  }
  // the subnets Docker gives the networks that are made without one: 172.18.0.0/16, then 172.19.0.0/16, ...
  let nextSubnet = 18
  const network = (n: Network & Partial<NetworkMade>): KeptNetwork => {
    const driver = n.driver ?? 'bridge'
    // Docker gives a bridge network addresses of its own when it is given none; `host` and `none` have none
    let subnet = n.subnet ?? ''
    let gateway = n.gateway ?? ''
    if (subnet === '' && driver !== 'host' && driver !== 'null') {
      const second = nextSubnet++
      subnet = `172.${second}.0.0/16`
      gateway = `172.${second}.0.1`
    } else if (subnet !== '' && gateway === '') {
      const first = range(subnet)
      if (first) gateway = [24, 16, 8, 0].map((shift) => ((first[0] + 1) >>> shift) & 255).join('.')
    }
    return { id: digest(`network:${n.name}`), name: n.name, driver, subnet, gateway, internal: n.internal ?? false, attachable: n.attachable ?? false, labels: { ...n.labels } }
  }
  /** Docker's own networks, which every Docker has and none can remove. */
  const OWN = ['bridge', 'host', 'none']
  const firstNetworks = (): KeptNetwork[] => {
    nextSubnet = 18
    return [
      network({ name: 'bridge', subnet: '172.17.0.0/16', gateway: '172.17.0.1' }),
      network({ name: 'host', driver: 'host' }),
      network({ name: 'none', driver: 'null' }),
      ...(options.networks ?? []).map(network)
    ]
  }
  const networkBy = (key: string): KeptNetwork | undefined => networks.find((n) => n.name === key || n.id === key || (key.length >= 12 && n.id.startsWith(key)))

  interface KeptVolume extends Volume {
    /** Docker named it: it is a container's own, made with it (`docker rm -v` removes it). */
    anonymous: boolean
    /** How much it holds, in bytes. */
    size: number
  }
  const volume = (name: string, anonymous: boolean, size = 0, driver = 'local', labels: Record<string, string> = {}): KeptVolume => ({ name, driver, labels, anonymous, size })
  const firstVolumes = (kept: Kept[]): KeptVolume[] =>
    kept.flatMap((c) => c.mounts.filter((m) => m.type === 'volume').map((m) => volume(m.source, false, 24_000_000)))

  let containers = seeds()
  let networks = firstNetworks()
  let volumes = firstVolumes(containers)
  let pulled: string[] = []
  let started: string[] = []
  let uploads = new Map<string, Upload[]>()
  // clients attached and not yet taken by a journey, and journeys waiting for one, by container name
  const unclaimed = new Map<string, Attached[]>()
  const claiming = new Map<string, ((attached: Attached) => void)[]>()
  const attachments = new Set<Socket>()
  let portTaken = false
  // the app's backend, reading `/events`, with what it asked for (Docker's `type`, `event` and `label` filters)
  const listeners = new Map<ServerResponse, { type?: string[]; event?: string[]; label?: string[] }>()

  /** Report a change on `/events`, to each listener that asked for its type, its action and its labels. */
  const report = (type: string, action: string, id: string, attributes: Record<string, string>): void => {
    const event = JSON.stringify({ Type: type, Action: action, Actor: { ID: id, Attributes: attributes }, time: 1 })
    for (const [listener, wanted] of listeners) {
      if (wanted.type && !wanted.type.includes(type)) continue
      if (wanted.event && !wanted.event.includes(action)) continue
      if (wanted.label && !labelled(attributes, wanted.label)) continue
      listener.write(`${event}\n`)
    }
  }
  const tell = (action: string, c: Kept): void => report('container', action, c.id, { name: c.name, ...c.labels })
  const find = (key: string): Kept | undefined => containers.find((c) => c.id === key || c.name === key)
  /** The containers made from an image, running or not. */
  const madeFrom = (image: KeptImage): Kept[] => containers.filter((c) => c.imageId === image.id)
  /** The containers that mount a volume, running or not. */
  const mounting = (name: string): Kept[] => containers.filter((c) => c.mounts.some((m) => m.type === 'volume' && m.source === name))
  // the ports Docker chooses, when a binding leaves it to Docker
  let nextPort = 49153
  /** The machine's port of each binding: the one asked for, or the one Docker chose. */
  const published = (c: Kept): { key: string; ip: string; port: string }[] =>
    Object.entries(c.bindings).flatMap(([key, bindings]) =>
      bindings.map((binding, i) => ({ key, ip: binding.HostIp, port: binding.HostPort === '' ? (c.chosen[`${key}#${i}`] ?? '') : binding.HostPort }))
    )
  type Filters = Record<string, string[] | undefined>
  const filtersOf = (query: URLSearchParams): Filters => JSON.parse(query.get('filters') ?? '{}') as Filters
  /** Docker's `label` filter: `key`, or `key=value`, every one of them. */
  const labelled = (labels: Record<string, string>, wanted: string[]): boolean =>
    wanted.every((label) => {
      const at = label.indexOf('=')
      const key = at === -1 ? label : label.slice(0, at)
      return key in labels && (at === -1 || labels[key] === label.slice(at + 1))
    })
  /** A boolean filter (`dangling`, `all`) as Docker reads it: `true` or `1`. */
  const yes = (values: string[] | undefined): boolean | undefined => (values === undefined ? undefined : values.some((v) => v === 'true' || v === '1'))

  const json = (res: ServerResponse, status: number, body: unknown): void => {
    res.writeHead(status, { 'content-type': 'application/json' }).end(JSON.stringify(body))
  }

  /** One frame of Docker's multiplexed output: the stream (1 is its output, 2 its errors), three zero bytes, the length, the bytes. */
  const framed = (data: Buffer, to: number): Buffer => {
    const head = Buffer.alloc(8)
    head[0] = to
    head.writeUInt32BE(data.length, 4)
    return Buffer.concat([head, data])
  }
  const frame = (line: string): Buffer => framed(Buffer.from(`${line}\n`), stream)

  /** An image as Docker lists it: the client reads every one of these fields. Docker counts its containers only for `/system/df`. */
  const listedImage = (image: KeptImage): object => ({
    Id: image.id,
    ParentId: '',
    RepoTags: image.tags,
    RepoDigests: [],
    Created: image.created,
    Size: image.size,
    SharedSize: -1,
    Labels: {},
    Containers: -1
  })

  /** Remove an image as Docker does, by a name or an id: Docker's records of what it untagged and deleted, or its refusal. */
  function removeImage(key: string, force: boolean): { status: number; body: unknown } {
    const found = imageBy(key)
    if (!found) return { status: 404, body: { message: `No such image: ${key}` } }
    const { image, tag } = found
    const users = madeFrom(image)
    const running = users.find((c) => c.running)
    const stopped = users.find((c) => !c.running)
    const records: object[] = []
    const untag = (name: string): void => {
      image.tags = image.tags.filter((t) => t !== name)
      records.push({ Untagged: name })
      report('image', 'untag', image.id, { name })
    }
    const remove = (): void => {
      images = images.filter((i) => i !== image)
      records.push({ Deleted: image.id })
      report('image', 'delete', image.id, { name: image.id })
    }
    if (tag !== undefined) {
      // a name the image has others beside goes alone; its last name goes only when no container needs it, or forced
      const user = users[0]
      if (!force && image.tags.length === 1 && user) {
        return {
          status: 409,
          body: { message: `conflict: unable to remove repository reference "${tag}" (must force) - container ${short(user.id)} is using its referenced image ${short(image.id)}` }
        }
      }
      untag(tag)
      // a running container keeps the image it runs: it is left without a name
      if (image.tags.length === 0 && !running) remove()
      return { status: 200, body: records }
    }
    // by its id: every name goes with it, though names of several repositories only when forced
    const repositories = new Set(image.tags.map((t) => t.slice(0, t.lastIndexOf(':'))))
    if (!force && repositories.size > 1) {
      return { status: 409, body: { message: `conflict: unable to delete ${short(image.id)} (must be forced) - image is referenced in multiple repositories` } }
    }
    if (running) return { status: 409, body: { message: `conflict: unable to delete ${short(image.id)} (cannot be forced) - image is being used by running container ${short(running.id)}` } }
    if (stopped && !force) return { status: 409, body: { message: `conflict: unable to delete ${short(image.id)} (must be forced) - image is being used by stopped container ${short(stopped.id)}` } }
    for (const name of [...image.tags]) untag(name)
    remove()
    return { status: 200, body: records }
  }

  const stateOf = (c: Kept): string => (c.running ? 'running' : c.exited || c.seeded ? 'exited' : 'created')
  const statusOf = (c: Kept): string => {
    if (c.running) return c.seeded ? 'Up 2 hours' : 'Up 1 second'
    if (c.seeded && !c.exited) return 'Exited (0) 3 days ago'
    return c.exited ? 'Exited (137) 1 second ago' : 'Created'
  }

  /** A container's mounts as Docker reports them, in its list and its inspection. */
  const mountsOf = (c: Kept): object[] =>
    c.mounts.map((m) =>
      m.type === 'volume'
        ? { Type: 'volume', Name: m.source, Source: `/var/lib/docker/volumes/${m.source}/_data`, Destination: m.target, Driver: 'local', Mode: 'z', RW: !m.readOnly, Propagation: '' }
        : { Type: 'bind', Source: m.source, Destination: m.target, Mode: '', RW: !m.readOnly, Propagation: 'rprivate' }
    )
  /** The networks a container is on, as Docker reports them: a stopped one keeps its network, without an address. */
  const endpointsOf = (c: Kept): object => {
    const on = networkBy(c.network)
    return { [c.network]: { NetworkID: on?.id ?? '', EndpointID: c.running ? c.id : '', IPAddress: c.running ? '172.17.0.2' : '' } }
  }

  const summaryOf = (c: Kept): object => {
    const image = images.find((i) => i.id === c.imageId)
    return {
      Id: c.id,
      Names: [`/${c.name}`],
      // the name it was made from while that name is still its image's; else the image's id
      Image: image?.tags.includes(c.image) ? c.image : c.imageId,
      ImageID: c.imageId,
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
        : [],
      HostConfig: { NetworkMode: c.network },
      NetworkSettings: { Networks: endpointsOf(c) },
      Mounts: mountsOf(c)
    }
  }

  /** Docker's inspection of a container: what it was made with (`Config`, `HostConfig`), and what it has now. */
  const inspectOf = (c: Kept): object => ({
    Id: c.id,
    Name: `/${c.name}`,
    Created: '2026-11-27T09:00:00.000000000Z',
    Path: c.entrypoint[0] ?? c.cmd[0] ?? '',
    Args: c.entrypoint.length > 0 ? [...c.entrypoint.slice(1), ...c.cmd] : c.cmd.slice(1),
    State: { Status: stateOf(c), Running: c.running, Paused: false, Restarting: false, ExitCode: c.exited ? 137 : 0 },
    Image: c.imageId,
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
    Mounts: mountsOf(c),
    NetworkSettings: {
      Ports: c.running
        ? Object.fromEntries(Object.keys(c.bindings).map((key) => [key, published(c).filter((p) => p.key === key).map((p) => ({ HostIp: p.ip === '' ? '0.0.0.0' : p.ip, HostPort: p.port }))]))
        : {},
      Networks: endpointsOf(c)
    }
  })

  /** A volume as Docker lists it: the client reads every one of these fields. */
  const volumeOf = (v: KeptVolume): object => ({
    Name: v.name,
    Driver: v.driver,
    Mountpoint: `/var/lib/docker/volumes/${v.name}/_data`,
    CreatedAt: '2026-11-27T09:00:00Z',
    Labels: v.labels,
    Scope: 'local',
    Options: {}
  })

  /** A network as Docker lists it. Its `Containers` are only filled in by an inspection of it. */
  const networkOf = (n: KeptNetwork): object => ({
    Name: n.name,
    Id: n.id,
    Created: '2026-11-27T09:00:00.000000000Z',
    Scope: n.driver === 'overlay' ? 'swarm' : 'local',
    Driver: n.driver,
    EnableIPv4: true,
    EnableIPv6: false,
    IPAM: { Driver: 'default', Options: {}, Config: n.subnet === '' ? [] : [{ Subnet: n.subnet, Gateway: n.gateway }] },
    Internal: n.internal,
    Attachable: n.attachable,
    Ingress: false,
    ConfigFrom: { Network: '' },
    ConfigOnly: false,
    Containers: {},
    Options: {},
    Labels: n.labels
  })

  /** A whole path, as Docker takes one for a mount. */
  const absolute = (path: string): boolean => path.startsWith('/') || /^[A-Za-z]:[\\/]/.test(path)
  // Docker's own rule for the names of containers, volumes and networks
  const NAME = /^[a-zA-Z0-9][a-zA-Z0-9_.-]+$/

  async function answer(req: IncomingMessage, res: ServerResponse): Promise<void> {
    const url = new URL(req.url ?? '/', 'http://docker')
    // a client may name the API version first (`/v1.47/containers/json`)
    const path = url.pathname.replace(/^\/v\d+\.\d+/, '')
    const method = req.method ?? 'GET'
    const flag = (name: string): boolean => ['1', 'true'].includes(url.searchParams.get(name) ?? '')

    if (path === '/version') return json(res, 200, { Version: '27.3.1', ApiVersion: '1.47', Os: 'linux', Arch: 'arm64' })
    if (path === '/_ping') return void res.writeHead(200, { 'content-type': 'text/plain' }).end('OK')
    if (path === '/events') {
      const { type, event, label } = filtersOf(url.searchParams)
      res.writeHead(200, { 'content-type': 'application/json' })
      res.flushHeaders()
      listeners.set(res, { type, event, label })
      req.on('close', () => listeners.delete(res))
      return
    }
    if (method === 'GET' && path === '/images/json') {
      const filters = filtersOf(url.searchParams)
      const wanted = filters.reference ?? []
      const dangling = yes(filters.dangling)
      return json(
        res,
        200,
        images
          .filter((image) => wanted.length === 0 || image.tags.some((tag) => wanted.includes(tag)))
          .filter((image) => dangling === undefined || dangling === (image.tags.length === 0))
          .map(listedImage)
      )
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
      pulled.push(image)
      res.writeHead(200, { 'content-type': 'application/json' })
      const step = (status: string, id?: string, progressDetail?: { current: number; total: number }): void =>
        void res.write(`${JSON.stringify({ status, ...(id === undefined ? {} : { id }), ...(progressDetail ? { progressDetail } : {}) })}\n`)
      step(`Pulling from ${from}`, tag ?? 'latest')
      // the registry has nothing newer than what is there
      if (images.some((i) => i.tags.includes(image))) {
        step(`Digest: sha256:${'e'.repeat(64)}`)
        report('image', 'pull', image, { name: image })
        return void res.end(`${JSON.stringify({ status: `Status: Image is up to date for ${image}` })}\n`)
      }
      // two layers, each downloaded in two steps and extracted, as Docker reports a pull
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
      images.push(imageOf([image]))
      report('image', 'pull', image, { name: image })
      return void res.end(`${JSON.stringify({ status: `Status: Downloaded newer image for ${image}` })}\n`)
    }
    if (method === 'POST' && path === '/images/prune') {
      // the dangling images by default, every unused one with `dangling=false`; never one a container was made from
      const onlyDangling = yes(filtersOf(url.searchParams).dangling) ?? true
      const doomed = images.filter((i) => (!onlyDangling || i.tags.length === 0) && madeFrom(i).length === 0)
      const deleted: object[] = []
      for (const image of doomed) {
        for (const name of image.tags) deleted.push({ Untagged: name })
        // Docker lists the layers it deleted beside the image
        deleted.push({ Deleted: image.id }, { Deleted: `sha256:${digest(`layer:${image.id}`)}` })
        report('image', 'delete', image.id, { name: image.id })
      }
      images = images.filter((i) => !doomed.includes(i))
      const space = doomed.reduce((sum, i) => sum + i.size, 0)
      report('image', 'prune', '', { reclaimed: String(space) })
      return json(res, 200, { ImagesDeleted: deleted.length === 0 ? null : deleted, SpaceReclaimed: space })
    }
    const imagePath = /^\/images\/(.+)$/.exec(path)
    if (method === 'DELETE' && imagePath) {
      const removed = removeImage(decodeURIComponent(imagePath[1]!), flag('force'))
      return json(res, removed.status, removed.body)
    }
    if (method === 'POST' && path === '/build') {
      // the context is read and not looked into: the image is the one the build names
      await bytesOf(req)
      const named = url.searchParams.get('t')
      const image = named === null ? undefined : /:[^/]*$/.test(named) ? named : `${named}:latest`
      const built = image === undefined ? imageOf([], undefined, `build#${images.length}`) : (imageBy(image)?.image ?? imageOf([image]))
      const id = built.id
      res.writeHead(200, { 'content-type': 'application/json' })
      const say = (line: object): void => void res.write(`${JSON.stringify(line)}\n`)
      say({ stream: `Step 1/1 : FROM scratch\n` })
      say({ stream: ' ---> Using cache\n' })
      say({ aux: { ID: id } })
      say({ stream: `Successfully built ${id.slice(7, 19)}\n` })
      if (!images.includes(built)) images.push(built)
      if (image !== undefined) say({ stream: `Successfully tagged ${image}\n` })
      return void res.end()
    }
    if (method === 'GET' && path === '/networks') {
      const filters = filtersOf(url.searchParams)
      const named = filters.name ?? []
      return json(
        res,
        200,
        networks
          .filter((n) => labelled(n.labels, filters.label ?? []))
          .filter((n) => named.length === 0 || named.some((name) => n.name.includes(name)))
          .map(networkOf)
      )
    }
    if (method === 'POST' && path === '/networks/create') {
      const body = (await bodyOf(req)) as {
        Name?: string
        Driver?: string
        Internal?: boolean
        Attachable?: boolean
        Labels?: Record<string, string>
        IPAM?: { Config?: { Subnet?: string; Gateway?: string }[] }
      }
      const name = body.Name ?? ''
      if (!NAME.test(name)) return json(res, 400, { message: `invalid name: "${name}" includes invalid characters, only "[a-zA-Z0-9][a-zA-Z0-9_.-]" are allowed` })
      if (networks.some((n) => n.name === name)) return json(res, 409, { message: `network with name ${name} already exists` })
      const driver = body.Driver || 'bridge'
      if (driver === 'host' || driver === 'null') return json(res, 403, { message: `only one instance of "${driver}" network is allowed` })
      if (driver === 'overlay') {
        return json(res, 503, { message: 'This node is not a swarm manager. Use "docker swarm init" or "docker swarm join" to connect this node to swarm and try again.' })
      }
      if (!['bridge', 'macvlan', 'ipvlan'].includes(driver)) return json(res, 404, { message: `plugin "${driver}" not found` })
      const config = body.IPAM?.Config?.[0]
      let subnet = config?.Subnet ?? ''
      /** Another network has an address of `asked`. */
      const overlaps = (asked: [number, number]): boolean =>
        networks.some((n) => {
          const theirs = range(n.subnet)
          return theirs !== undefined && asked[0] <= theirs[1] && theirs[0] <= asked[1]
        })
      if (subnet !== '') {
        const asked = range(subnet)
        if (!asked) return json(res, 400, { message: `invalid CIDR address: ${subnet}` })
        if (overlaps(asked)) return json(res, 403, { message: 'Pool overlaps with other one on this address space' })
      } else {
        // Docker gives it the next of its own subnets that no network has
        while (overlaps(range(`172.${nextSubnet}.0.0/16`)!)) nextSubnet++
        subnet = `172.${nextSubnet++}.0.0/16`
      }
      const kept = network({
        name,
        driver,
        subnet,
        gateway: config?.Gateway ?? '',
        internal: body.Internal === true,
        attachable: body.Attachable === true,
        labels: body.Labels ?? {}
      })
      networks.push(kept)
      report('network', 'create', kept.id, { name, type: driver })
      return json(res, 201, { Id: kept.id, Warning: '' })
    }
    const networkPath = /^\/networks\/([^/]+)$/.exec(path)
    if (method === 'DELETE' && networkPath) {
      const key = decodeURIComponent(networkPath[1]!)
      const found = networkBy(key)
      if (!found) return json(res, 404, { message: `network ${key} not found` })
      if (OWN.includes(found.name)) return json(res, 403, { message: `${found.name} is a pre-defined network and cannot be removed` })
      // a stopped container keeps its place on the network, and does not hold it
      if (containers.some((c) => c.running && c.network === found.name)) {
        return json(res, 403, { message: `error while removing network: network ${found.name} id ${found.id} has active endpoints` })
      }
      networks = networks.filter((n) => n !== found)
      report('network', 'destroy', found.id, { name: found.name, type: found.driver })
      return void res.writeHead(204).end()
    }
    if (method === 'GET' && path === '/volumes') {
      const filters = filtersOf(url.searchParams)
      const named = filters.name ?? []
      const dangling = yes(filters.dangling)
      return json(res, 200, {
        Volumes: volumes
          .filter((v) => named.length === 0 || named.some((name) => v.name.includes(name)))
          .filter((v) => labelled(v.labels, filters.label ?? []))
          .filter((v) => dangling === undefined || dangling === (mounting(v.name).length === 0))
          .map(volumeOf),
        Warnings: []
      })
    }
    if (method === 'POST' && path === '/volumes/create') {
      const body = (await bodyOf(req)) as { Name?: string; Driver?: string; Labels?: Record<string, string> }
      const name = body.Name || digest(`volume:${made}:${volumes.length}`)
      if (!NAME.test(name)) {
        return json(res, 400, {
          message: `create ${name}: "${name}" includes invalid characters for a local volume name, only "[a-zA-Z0-9][a-zA-Z0-9_.-]" are allowed. If you intended to pass a host directory, use absolute path`
        })
      }
      const driver = body.Driver || 'local'
      if (driver !== 'local') return json(res, 404, { message: `create ${name}: error looking up volume plugin ${driver}: plugin "${driver}" not found` })
      // a volume that is there is the answer: Docker makes nothing
      const there = volumes.find((v) => v.name === name)
      if (there) return json(res, 201, volumeOf(there))
      const kept = volume(name, !body.Name, 0, driver, body.Labels ?? {})
      volumes.push(kept)
      report('volume', 'create', name, { driver })
      return json(res, 201, volumeOf(kept))
    }
    if (method === 'POST' && path === '/volumes/prune') {
      // since API 1.42 only the anonymous volumes, unless `all`; never one a container mounts, running or not
      const all = yes(filtersOf(url.searchParams).all) ?? false
      const doomed = volumes.filter((v) => (all || v.anonymous) && mounting(v.name).length === 0)
      volumes = volumes.filter((v) => !doomed.includes(v))
      for (const v of doomed) report('volume', 'destroy', v.name, { driver: v.driver })
      const space = doomed.reduce((sum, v) => sum + v.size, 0)
      report('volume', 'prune', '', { reclaimed: String(space) })
      return json(res, 200, { VolumesDeleted: doomed.length === 0 ? null : doomed.map((v) => v.name), SpaceReclaimed: space })
    }
    const volumePath = /^\/volumes\/([^/]+)$/.exec(path)
    if (method === 'DELETE' && volumePath) {
      const name = decodeURIComponent(volumePath[1]!)
      const found = volumes.find((v) => v.name === name)
      if (!found) return json(res, 404, { message: `get ${name}: no such volume` })
      // a volume a container mounts stays, running or not, forced or not
      const users = mounting(name)
      if (users.length > 0) return json(res, 409, { message: `remove ${name}: volume is in use - [${users.map((c) => c.id).join(', ')}]` })
      volumes = volumes.filter((v) => v !== found)
      report('volume', 'destroy', name, { driver: found.driver })
      return void res.writeHead(204).end()
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
      if (!imageBy(image)) return json(res, 404, { message: `No such image: ${image}` })
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
      const on = networkBy(mode === '' || mode === 'default' ? 'bridge' : mode)
      if (!on) return json(res, 404, { message: `network ${mode} not found` })
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
          volumes.push(volume(m.source, true))
          report('volume', 'create', m.source, { driver: 'local' })
        } else if (!volumes.some((v) => v.name === m.source)) {
          volumes.push(volume(m.source, false))
          report('volume', 'create', m.source, { driver: 'local' })
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
          network: on.name,
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
      created.tty = body.Tty === true
      containers.push(created)
      tell('create', created)
      return json(res, 201, { Id: created.id, Warnings: [] })
    }

    const [, key, verb] = /^\/containers\/([^/]+)(?:\/(\w+))?$/.exec(path) ?? []
    if (key === undefined) return json(res, 404, { message: `the fake Docker has no ${method} ${path}` })
    const found = find(key)
    if (!found) return json(res, 404, { message: `No such container: ${key}` })

    if (method === 'GET' && verb === 'json') return json(res, 200, inspectOf(found))
    if (method === 'PUT' && verb === 'archive') {
      const at = url.searchParams.get('path') ?? ''
      if (!at.startsWith('/')) return json(res, 400, { message: `path ${at} is not absolute` })
      const archive = new Uint8Array(await bytesOf(req))
      uploads.set(found.name, [...(uploads.get(found.name) ?? []), { path: at, archive }])
      return void res.writeHead(200).end()
    }
    if (method === 'GET' && verb === 'logs') {
      const tail = Number(url.searchParams.get('tail') ?? 'all')
      const lines = Number.isNaN(tail) ? found.log : found.log.slice(-tail)
      return void res.writeHead(200, { 'content-type': 'application/vnd.docker.multiplexed-stream' }).end(Buffer.concat(lines.map(frame)))
    }
    if (method === 'DELETE' && verb === undefined) {
      if (found.running && !flag('force')) {
        return json(res, 409, {
          message: `cannot remove container "/${found.name}": container is running: stop the container before removing or force remove`
        })
      }
      containers = containers.filter((c) => c !== found)
      // its anonymous volumes go with it when asked; a named volume is never a container's own
      if (flag('v')) {
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
      // the network it was put on went while it was stopped
      if (!networkBy(found.network)) return json(res, 404, { message: `network ${found.network} not found` })
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

  /** A client attached to a container: the connection Docker hijacked, from the container's side. */
  const attach = (c: Kept, socket: Socket): Attached => {
    const chunks: (Uint8Array | null)[] = []
    const readers: ((chunk: Uint8Array | null) => void)[] = []
    const take = (chunk: Uint8Array | null): void => {
      const reader = readers.shift()
      if (reader) reader(chunk)
      else chunks.push(chunk)
    }
    socket.on('data', (data: Buffer) => take(new Uint8Array(data)))
    // the client closed the input (or the connection went): there is nothing more to read
    socket.on('end', () => take(null))
    socket.on('close', () => {
      attachments.delete(socket)
      take(null)
    })
    let ended = false
    return {
      name: c.name,
      read: () => {
        const chunk = chunks.shift()
        if (chunk !== undefined) return Promise.resolve(chunk)
        if (ended) return Promise.resolve(null)
        return new Promise((resolve) => readers.push(resolve))
      },
      write(data, to = 'stdout') {
        const bytes = typeof data === 'string' ? Buffer.from(data, 'utf8') : Buffer.from(data)
        socket.write(c.tty ? bytes : framed(bytes, to === 'stderr' ? 2 : 1))
      },
      end() {
        ended = true
        socket.end()
      }
    }
  }

  const server = createServer((req, res) => {
    answer(req, res).catch((e: unknown) => json(res, 500, { message: String(e) }))
  })
  // `POST /containers/{id}/attach` asks for the connection itself (`Upgrade: tcp`): Docker answers 101 and the
  // connection is the container's streams from then on
  server.on('upgrade', (req: IncomingMessage, socket: Socket) => {
    const url = new URL(req.url ?? '/', 'http://docker')
    const [, key] = /^(?:\/v\d+\.\d+)?\/containers\/([^/]+)\/attach$/.exec(url.pathname) ?? []
    const found = key === undefined ? undefined : find(key)
    if (req.method !== 'POST' || found === undefined) {
      const body = JSON.stringify({ message: key === undefined ? `the fake Docker has no ${req.method} ${url.pathname}` : `No such container: ${key}` })
      return void socket.end(`HTTP/1.1 404 Not Found\r\nContent-Type: application/json\r\nContent-Length: ${Buffer.byteLength(body)}\r\n\r\n${body}`)
    }
    attachments.add(socket)
    socket.write('HTTP/1.1 101 UPGRADED\r\nContent-Type: application/vnd.docker.raw-stream\r\nConnection: Upgrade\r\nUpgrade: tcp\r\n\r\n')
    const attached = attach(found, socket)
    const waiting = claiming.get(found.name)?.shift()
    if (waiting) waiting(attached)
    else unclaimed.set(found.name, [...(unclaimed.get(found.name) ?? []), attached])
  })
  await new Promise<void>((resolve) => server.listen(0, '127.0.0.1', resolve))
  const { port } = server.address() as AddressInfo

  return {
    host: `tcp://127.0.0.1:${port}`,
    reset() {
      images = firstImages()
      containers = seeds()
      networks = firstNetworks()
      volumes = firstVolumes(containers)
      nextPort = 49153
      pulled = []
      started = []
      uploads = new Map()
      unclaimed.clear()
      for (const socket of attachments) socket.destroy()
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
    network(name) {
      const n = networks.find((it) => it.name === name)
      return n && { name: n.name, driver: n.driver, subnet: n.subnet, gateway: n.gateway, internal: n.internal, attachable: n.attachable, labels: { ...n.labels } }
    },
    volumes: () => volumes.map((v) => v.name),
    volume(name) {
      const v = volumes.find((it) => it.name === name)
      return v && { name: v.name, driver: v.driver, labels: { ...v.labels } }
    },
    images: () => images.map((i) => ({ id: i.id, tags: [...i.tags], size: i.size })),
    uploads: (name) => [...(uploads.get(name) ?? [])],
    attached(name) {
      const ready = unclaimed.get(name)?.shift()
      if (ready) return Promise.resolve(ready)
      return new Promise((resolve) => claiming.set(name, [...(claiming.get(name) ?? []), resolve]))
    },
    started: () => [...started],
    takePort(taken) {
      portTaken = taken
    },
    run(name, image) {
      if (!imageBy(image)) images.push(imageOf([image]))
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
        for (const listener of listeners.keys()) listener.end()
        for (const socket of attachments) socket.destroy()
        server.closeAllConnections()
        server.close(() => resolve())
      })
  }
}
