/**
 * A stand-in for the Containers app's backend, for stories and unit tests: a Docker kept in the page, with no host and
 * no daemon. It has containers that start, stop and go away as the real ones do, stacks worked out from them as the
 * backend works them out, and a line of log for everything that happens to a container. Containers are made from a
 * spec as the backend makes them: the image pulled first when it is not there (with its progress told), the spec
 * checked for some of what the backend checks, in its words.
 */
type Container = VosApp.Containers.Container
type Details = VosApp.Containers.Details
type Network = VosApp.Containers.Network
type Overview = VosApp.Containers.Overview
type Port = VosApp.Containers.Port
type Pulling = VosApp.Containers.PullingParams
type Spec = VosApp.Containers.Spec
type Stack = VosApp.Containers.Stack
type State = VosApp.Containers.State
type Volume = VosApp.Containers.Volume
import type { Action, ContainersEvents, ContainersRpc } from './containers.svelte'

export interface FakeBackend {
  readonly rpc: ContainersRpc
  readonly events: ContainersEvents
  /** Send an event to the window, as the host does (`host:reconnected`) or the backend's watcher (`changed`). */
  emit(name: string, data?: unknown): void
  /** Docker changes behind the app's back, as a terminal would change it: the windows are told. */
  outside(change: (containers: Container[]) => Container[]): void
  /** Docker goes away, or comes back. */
  reach(reachable: boolean): void
  /** A container writes a line. */
  write(container: string, line: string): void
}

export interface FakeOptions {
  /** The containers that are there (default: `CONTAINERS`). */
  containers?: readonly Container[]
  /** Docker cannot be reached, for this reason. */
  problem?: string
  /** The address that is chosen (default: this machine's own, empty). */
  host?: string
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
  /** The names of containers Docker refuses to act on, as it does when a port is taken. */
  refusing?: readonly string[]
  /** The images Docker has (default: those of the containers); any other is pulled first. */
  images?: readonly string[]
  /** Images the registry does not have: pulling one fails as Docker's does. */
  missingImages?: readonly string[]
  /** Docker's networks (default: its own, and one for each stack). */
  networks?: readonly Network[]
  /** Docker's volumes (default: one for each container). */
  volumes?: readonly Volume[]
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

const STATUS: Readonly<Record<State, string>> = {
  running: 'Up 2 hours',
  created: 'Created',
  exited: 'Exited (0) 3 days ago',
  paused: 'Up 2 hours (Paused)',
  restarting: 'Restarting (1) 4 seconds ago',
  removing: 'Removal In Progress',
  dead: 'Dead'
}

/** A container, for a story's or a test's starting point. Its id is its name, written out. */
export function container(
  name: string,
  image: string,
  state: State = 'running',
  extra: Partial<Pick<Container, 'ports' | 'stack' | 'service' | 'status'>> = {}
): Container {
  return { id: `id-${name}`, name, image, state, status: STATUS[state], ports: [], stack: '', service: '', ...extra }
}

/** A machine with a Compose stack that is up, one that is half up, and two containers on their own. */
export const CONTAINERS: readonly Container[] = [
  container('blog-db-1', 'postgres:16', 'running', { stack: 'blog', service: 'db' }),
  container('blog-web-1', 'ghost:5', 'running', { stack: 'blog', service: 'web', ports: ['8080:2368'] }),
  container('media-jellyfin-1', 'jellyfin/jellyfin:10.9', 'running', { stack: 'media', service: 'jellyfin', ports: ['8096:8096'] }),
  container('media-transcoder-1', 'linuxserver/ffmpeg:7', 'exited', { stack: 'media', service: 'transcoder' }),
  container('pihole', 'pihole/pihole:2024.07.0', 'running', { ports: ['53:53/udp', '8053:80'] }),
  container('scratch', 'alpine:3.20', 'exited')
]

const FOLDERS: Readonly<Record<string, string>> = { blog: '/srv/blog', media: '/srv/media' }

/** A container's published port (`8080:80`, `53:53/udp`) as a row of its spec. */
const portOf = (label: string): Port => {
  const [host = '', rest = ''] = label.split(':')
  const [container = '', protocol] = rest.split('/')
  return { host_ip: '', host_port: host, container_port: container, protocol: protocol === 'udp' ? 'udp' : 'tcp' }
}

/** The spec a container of the fake was made with, worked out from what is listed of it. */
export function specOf(c: Container): Spec {
  return {
    image: c.image,
    name: c.name,
    command: c.image.startsWith('postgres') ? 'postgres' : '',
    entrypoint: '',
    environment: [
      { name: 'PATH', value: '/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin' },
      { name: 'TZ', value: 'Europe/Amsterdam' },
      ...(c.image.startsWith('postgres') ? [{ name: 'POSTGRES_PASSWORD', value: 'correct-horse' }] : [])
    ],
    ports: c.ports.map(portOf),
    publish_all: false,
    mounts: [{ kind: 'volume', source: `${c.name}-data`, target: '/data', read_only: false }],
    network: c.stack === '' ? 'bridge' : `${c.stack}_default`,
    restart: c.stack === '' ? 'no' : 'unless-stopped',
    retries: '',
    labels: c.stack === '' ? [] : [
      { name: 'com.docker.compose.project', value: c.stack },
      { name: 'com.docker.compose.service', value: c.service }
    ],
    working_dir: '',
    user: '',
    hostname: '',
    start: c.state === 'running'
  }
}

/** Some of what the backend refuses in a spec, in its words: enough for a story to show a refusal. */
export function refusal(spec: Spec): string | null {
  const image = spec.image.trim()
  if (image === '') return 'Say which image the container runs, like nginx:1.27.'
  if (!/^[a-z0-9]+([._/-][a-z0-9]+)*(:[\w][\w.-]*)?$/.test(image)) {
    return `${image} is not the name of an image. An image is named like nginx, nginx:1.27 or ghcr.io/owner/app:1.0.`
  }
  const port = (text: string): boolean => /^\d+$/.test(text) && Number(text) >= 1 && Number(text) <= 65535
  for (const [i, p] of spec.ports.entries()) {
    const which = spec.ports.length === 1 ? 'Port' : `Port ${i + 1}`
    if (!port(p.container_port.trim())) return `${which}: the container's port must be a number from 1 to 65535.`
    if (p.host_port.trim() !== '' && !port(p.host_port.trim())) {
      return `${which}: the machine's port must be a number from 1 to 65535, or empty for one Docker chooses.`
    }
  }
  for (const [i, m] of spec.mounts.entries()) {
    if (!m.target.trim().startsWith('/')) {
      return `${spec.mounts.length === 1 ? 'Mount' : `Mount ${i + 1}`}: the place in the container is a whole path, like /data.`
    }
  }
  return null
}

/** The stacks a list of containers belongs to, by name: what the backend's stacks.luau answers. */
export function stacksOf(containers: readonly Container[]): Stack[] {
  const stacks = new Map<string, Stack>()
  for (const c of containers) {
    if (c.stack === '') continue
    const stack = stacks.get(c.stack) ?? { name: c.stack, running: 0, total: 0, folder: FOLDERS[c.stack] ?? '' }
    stacks.set(c.stack, { ...stack, total: stack.total + 1, running: stack.running + (c.state === 'running' ? 1 : 0) })
  }
  return [...stacks.values()].sort((a, b) => a.name.localeCompare(b.name))
}

const ADDRESS = /^(unix:\/\/\/|tcp:\/\/[^/]|https?:\/\/[^/])/

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let containers: Container[] = (options.containers ?? CONTAINERS).map((c) => ({ ...c, ports: [...c.ports] }))
  const specs = new Map<string, Spec>(containers.map((c) => [c.id, specOf(c)]))
  const images = new Set(options.images ?? containers.map((c) => c.image))
  const missing = new Set(options.missingImages ?? [])
  const networks: Network[] = [
    ...(options.networks ?? [
      { name: 'bridge', driver: 'bridge' },
      { name: 'host', driver: 'host' },
      { name: 'none', driver: 'null' },
      ...stacksOf(containers).map((s) => ({ name: `${s.name}_default`, driver: 'bridge' }))
    ])
  ]
  let volumes: Volume[] = [...(options.volumes ?? containers.map((c) => ({ name: `${c.name}-data`, driver: 'local' })))]
  let made = 0
  let problem = options.problem ?? ''
  let host = options.host ?? ''
  const pace = options.pace ?? 0
  const refusing = new Set(options.refusing ?? [])
  const written = new Map<string, string[]>(containers.map((c) => [c.id, [`${c.name} started`, 'listening']]))
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))
  const answered = <T>(answer: () => T): Promise<T> => new Promise((resolve, reject) => setTimeout(() => {
    try {
      resolve(answer())
    } catch (e) {
      reject(e as Error)
    }
  }, pace))

  const overview = (): Overview => {
    const reachable = problem === ''
    const sorted = [...containers].sort((a, b) => a.name.localeCompare(b.name))
    return {
      engine: { reachable, version: reachable ? '27.3.1' : '', host, problem },
      containers: reachable ? sorted.map((c) => ({ ...c, ports: [...c.ports] })) : [],
      stacks: reachable ? stacksOf(sorted) : []
    }
  }

  const find = (id: string): Container => {
    if (problem !== '') throw new Refused(problem)
    const found = containers.find((c) => c.id === id)
    if (!found) throw new Refused(`No such container: ${id}`)
    return found
  }
  const log = (id: string, line: string): void => {
    written.set(id, [...(written.get(id) ?? []), line])
  }

  function act(found: Container, action: Action): void {
    if (refusing.has(found.name)) throw new Refused(`driver failed programming external connectivity on endpoint ${found.name}: port is already allocated`)
    if (action === 'remove') {
      containers = containers.filter((c) => c !== found)
      return
    }
    const state: State = action === 'stop' ? 'exited' : 'running'
    containers = containers.map((c) => (c === found ? { ...c, state, status: state === 'running' ? 'Up 1 second' : 'Exited (0) 1 second ago' } : c))
    log(found.id, action === 'stop' ? 'stopping' : `${found.name} started`)
  }

  /**
   * Make a container from a spec, as the backend does: checked, its image pulled (and the pull told) when Docker does
   * not have it, made by the name given or one Docker chooses, and started when the spec says so.
   */
  function make(spec: Spec, replacing?: Container): Container {
    if (problem !== '') throw new Refused(problem)
    const wrong = refusal(spec)
    if (wrong) throw new Refused(wrong)
    const image = spec.image.trim()
    if (!images.has(image)) {
      if (missing.has(image)) {
        throw new Refused(`The image ${image} could not be pulled: pull access denied for ${image.split(':')[0]}, repository does not exist or may require 'docker login'`)
      }
      for (const current of [0, 24_000_000, 48_000_000]) emit('pulling', { image, status: current === 0 ? 'Pulling fs layer' : 'Downloading', current, total: 48_000_000 } satisfies Pulling)
      images.add(image)
    }
    made += 1
    const name = spec.name.trim() || `quirky_${made}`
    if (containers.some((c) => c.name === name && c !== replacing)) {
      throw new Refused(`Conflict. The container name "/${name}" is already in use by container "id-${name}". You have to remove (or rename) that container to be able to reuse that name.`)
    }
    const labels = Object.fromEntries(spec.labels.map((l) => [l.name, l.value]))
    const fresh: Container = {
      id: `id-${name}-${made}`,
      name,
      image,
      state: spec.start ? 'running' : 'created',
      status: spec.start ? 'Up 1 second' : 'Created',
      ports: spec.ports.filter((p) => p.host_port.trim() !== '').map((p) => `${p.host_port.trim()}:${p.container_port.trim()}${p.protocol === 'udp' ? '/udp' : ''}`),
      stack: labels['com.docker.compose.project'] ?? '',
      service: labels['com.docker.compose.service'] ?? ''
    }
    for (const m of spec.mounts) {
      if (m.kind === 'volume' && m.source.trim() !== '' && !volumes.some((v) => v.name === m.source.trim())) {
        volumes = [...volumes, { name: m.source.trim(), driver: 'local' }]
      }
    }
    specs.set(fresh.id, { ...spec, name, image })
    written.set(fresh.id, spec.start ? [`${name} started`] : [])
    return fresh
  }

  const onContainer = (action: Action) => ({ container: id }: { container: string }) =>
    answered(() => {
      act(find(id), action)
      return overview()
    })
  const onStack = (action: Action) => ({ stack }: { stack: string }) =>
    answered(() => {
      if (problem !== '') throw new Refused(problem)
      const members = containers.filter((c) => c.stack === stack)
      if (members.length === 0) throw new Refused('There is no such stack.')
      // it goes on past a container that fails, and answers with the first failure
      let failure: unknown
      for (const member of members) {
        try {
          act(member, action)
        } catch (e) {
          failure ??= e
        }
      }
      if (failure !== undefined) throw failure as Error
      return overview()
    })

  const rpc: ContainersRpc = {
    overview: () => answered(overview),
    connect: ({ host: address }) =>
      answered(() => {
        if (address !== '' && !ADDRESS.test(address)) {
          throw new Refused("The address must start with unix:///, tcp:// or http://, or be empty for this machine's own Docker.")
        }
        host = address
        // this machine's own Docker is there; nothing listens anywhere else
        problem = address === '' ? '' : `Docker cannot be reached at ${address}.`
        return overview()
      }),
    start: onContainer('start'),
    stop: onContainer('stop'),
    restart: onContainer('restart'),
    remove: ({ container: id, force }) =>
      answered(() => {
        const found = find(id)
        if (found.state === 'running' && !force) {
          throw new Refused(`cannot remove container "/${found.name}": container is running: stop the container before removing or force remove`)
        }
        containers = containers.filter((c) => c !== found)
        return overview()
      }),
    create: ({ spec }) =>
      answered(() => {
        const fresh = make(spec)
        containers = [...containers, fresh]
        return { container: fresh.id, overview: overview() }
      }),
    recreate: ({ container: id, spec }) =>
      answered(() => {
        const old = find(id)
        const fresh = make(spec, old)
        containers = containers.map((c) => (c === old ? fresh : c))
        return { container: fresh.id, overview: overview() }
      }),
    networks: () =>
      answered(() => {
        if (problem !== '') throw new Refused(problem)
        return [...networks]
      }),
    volumes: () =>
      answered(() => {
        if (problem !== '') throw new Refused(problem)
        return [...volumes].sort((a, b) => a.name.localeCompare(b.name))
      }),
    startStack: onStack('start'),
    stopStack: onStack('stop'),
    restartStack: onStack('restart'),
    removeStack: onStack('remove'),
    details: ({ container: id }) =>
      answered((): Details => {
        const found = find(id)
        const spec = specs.get(id) ?? specOf(found)
        return {
          container: id,
          command: found.image.startsWith('postgres') ? 'docker-entrypoint.sh postgres' : '/init',
          health: found.image.startsWith('postgres') && found.state === 'running' ? 'healthy' : '',
          mounts: spec.mounts.map((m) => `${m.source} -> ${m.target}${m.read_only ? ' (read-only)' : ''}`).sort(),
          networks: [spec.network],
          spec: { ...spec, start: found.state === 'running' }
        }
      }),
    logs: ({ container: id, tail }) =>
      answered(() => {
        find(id)
        const lines = written.get(id) ?? []
        const shown = lines.slice(-(tail ?? 200))
        return { container: id, text: shown.map((line) => `${line}\n`).join(''), truncated: shown.length < lines.length }
      })
  }

  const events: ContainersEvents = {
    on(name: string, fn: (data: never) => void): () => void {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn as (data: unknown) => void)
      return () => heard.delete(fn as (data: unknown) => void)
    }
  }

  return {
    rpc,
    events,
    emit,
    outside(change) {
      containers = change(containers)
      emit('changed')
    },
    reach(reachable) {
      problem = reachable ? '' : 'Docker is not running on this machine, or cannot be reached.'
      emit('changed')
    },
    write(name, line) {
      const found = containers.find((c) => c.name === name)
      if (found) log(found.id, line)
    }
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      overview: refuse,
      connect: refuse,
      start: refuse,
      stop: refuse,
      restart: refuse,
      remove: refuse,
      create: refuse,
      recreate: refuse,
      networks: refuse,
      volumes: refuse,
      startStack: refuse,
      stopStack: refuse,
      restartStack: refuse,
      removeStack: refuse,
      details: refuse,
      logs: refuse
    },
    events: { on: () => () => {} },
    emit: () => {},
    outside: () => {},
    reach: () => {},
    write: () => {}
  }
}
