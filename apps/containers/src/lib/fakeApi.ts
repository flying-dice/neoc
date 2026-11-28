/**
 * A stand-in for the Containers app's backend, for stories and unit tests: a Docker kept in the page, with no host and
 * no daemon. It has containers that start, stop and go away as the real ones do, stacks worked out from them as the
 * backend works them out, and a line of log for everything that happens to a container. Containers are made from a
 * spec as the backend makes them: the image pulled first when it is not there (with its progress told), the spec
 * checked for some of what the backend checks, in its words. Its images, volumes and networks know the containers that
 * use them, and are refused what Docker refuses: an image a container was made from, unless forced (never one a running
 * container was made from), a volume a container uses, a network a running container is on, Docker's own networks.
 * It keeps image recipes, and builds one as the backend does: its lines told as it goes (`building`), a Dockerfile with
 * a `RUN exit <n>` failing there as Docker fails it, and the name moved to the new image.
 */
type Container = VosApp.Containers.Container
type Details = VosApp.Containers.Details
type Image = VosApp.Containers.Image
type Network = VosApp.Containers.Network
type Overview = VosApp.Containers.Overview
type Port = VosApp.Containers.Port
type Pulling = VosApp.Containers.PullingParams
type Recipe = VosApp.Containers.Recipe
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
  /** End the builds held (`holdBuilds`). */
  finish(): void
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
  /** The images Docker has (default: `IMAGES`, those of the containers and two more); any other is pulled first. */
  images?: readonly Kept<Image>[]
  /** Images the registry does not have: pulling one fails as Docker's does. */
  missingImages?: readonly string[]
  /** Docker's networks (default: its own, and one for each stack). */
  networks?: readonly Kept<Network>[]
  /** Docker's volumes (default: one for each container). */
  volumes?: readonly Kept<Volume>[]
  /** The image recipes kept (default: none). */
  recipes?: readonly Recipe[]
  /** A build does not end until `finish()`: the window shows it under way. */
  holdBuilds?: boolean
}

/** An image, a volume or a network as the fake keeps it: the containers that use it are worked out when it is listed. */
export type Kept<T> = Omit<T, 'containers'>

/** A recipe of the fake, as the backend lists it. */
export const recipe = (tag: string, more: Partial<Recipe> = {}): Recipe => ({
  id: `recipe-${tag}`,
  tag,
  dockerfile: 'FROM alpine:3.20\nRUN apk add --no-cache curl',
  image: '',
  current: false,
  problem: '',
  building: false,
  ...more
})

/** The Docker message of a `RUN exit <n>` with n other than 0, as a build fails at it; null for a Dockerfile that builds. */
export function buildFailure(dockerfile: string): string | null {
  const m = /^\s*RUN\s+exit\s+([1-9]\d*)\s*$/im.exec(dockerfile)
  return m ? `The command '/bin/sh -c exit ${m[1]}' returned a non-zero code: ${m[1]}` : null
}

/** An image of the fake: its id and size follow from its names. */
export function image(tags: string[], more: Partial<Kept<Image>> = {}): Kept<Image> {
  let hash = 0
  for (const c of tags.join(',') || 'dangling') hash = (hash * 31 + c.charCodeAt(0)) >>> 0
  const hex = hash.toString(16).padStart(8, '0')
  return { id: `sha256:${hex.repeat(8)}`, tags, size: 20_000_000 + (hash % 400) * 1_000_000, created: 1_700_000_000 + (hash % 10_000_000), ...more }
}

/** A volume of the fake, kept by Docker's own driver. */
export const volume = (name: string, more: Partial<Kept<Volume>> = {}): Kept<Volume> => ({
  name,
  driver: 'local',
  mountpoint: `/var/lib/docker/volumes/${name}/_data`,
  created: '2026-11-27T09:00:00Z',
  ...more
})

/** A network of the fake: a bridge network on this machine, with addresses Docker chose, unless it says otherwise. */
export const network = (name: string, more: Partial<Kept<Network>> = {}): Kept<Network> => ({
  name,
  driver: 'bridge',
  scope: 'local',
  subnet: '',
  gateway: '',
  internal: false,
  attachable: false,
  predefined: false,
  ...more
})

/** Docker's own networks, which every Docker has and none can remove. */
export const OWN_NETWORKS: readonly Kept<Network>[] = [
  network('bridge', { subnet: '172.17.0.0/16', gateway: '172.17.0.1', predefined: true }),
  network('host', { driver: 'host', predefined: true }),
  network('none', { driver: 'null', predefined: true })
]

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

/** The images of a machine with these containers: one for each image they run, an unused one and a dangling one. */
export function imagesOf(containers: readonly Container[]): Kept<Image>[] {
  return [
    ...[...new Set(containers.map((c) => c.image))].map((name) => image([name])),
    image(['redis:7', 'redis:latest']),
    image([], { size: 140_000_000 })
  ]
}
/** The images of the machine of `CONTAINERS`. */
export const IMAGES: readonly Kept<Image>[] = imagesOf(CONTAINERS)

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let containers: Container[] = (options.containers ?? CONTAINERS).map((c) => ({ ...c, ports: [...c.ports] }))
  const specs = new Map<string, Spec>(containers.map((c) => [c.id, specOf(c)]))
  let images: Kept<Image>[] = (options.images ?? imagesOf(containers)).map((i) => ({ ...i, tags: [...i.tags] }))
  const missing = new Set(options.missingImages ?? [])
  let networks: Kept<Network>[] = [
    ...(options.networks ?? [
      ...OWN_NETWORKS,
      ...stacksOf(containers).map((s, i) => network(`${s.name}_default`, { subnet: `172.${18 + i}.0.0/16`, gateway: `172.${18 + i}.0.1` }))
    ])
  ]
  let volumes: Kept<Volume>[] = [...(options.volumes ?? containers.map((c) => volume(`${c.name}-data`)))]
  let made = 0
  let problem = options.problem ?? ''
  let host = options.host ?? ''
  const pace = options.pace ?? 0
  const refusing = new Set(options.refusing ?? [])
  const written = new Map<string, string[]>(containers.map((c) => [c.id, [`${c.name} started`, 'listening']]))
  const listeners = new Map<string, Set<(data: unknown) => void>>()
  let recipes: Recipe[] = (options.recipes ?? []).map((r) => ({ ...r }))
  let held: (() => void)[] = []
  // what each recipe's image was built from: its name and Dockerfile
  const builtFrom = new Map<string, string>()

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
   * Pull an image, as the backend does: the pull told as it goes (by the name it was asked by), or refused as Docker
   * refuses it. `nginx` pulls `nginx:latest`.
   */
  function pull(name: string): void {
    const tagged = /:[^/]*$/.test(name) ? name : `${name}:latest`
    if (missing.has(tagged)) {
      throw new Refused(`The image ${name} could not be pulled: pull access denied for ${tagged.split(':')[0]}, repository does not exist or may require 'docker login'`)
    }
    for (const current of [0, 24_000_000, 48_000_000]) emit('pulling', { image: name, status: current === 0 ? 'Pulling fs layer' : 'Downloading', current, total: 48_000_000 } satisfies Pulling)
    if (!images.some((i) => i.tags.includes(tagged))) images = [...images, image([tagged], { size: 48_000_000 })]
  }

  // the containers that use an image, a volume or a network, by name: worked out from what each was made with
  const madeFrom = (i: Kept<Image>): Container[] => containers.filter((c) => i.tags.includes(c.image) || c.image === i.id)
  const mounting = (name: string): Container[] => containers.filter((c) => (specs.get(c.id) ?? specOf(c)).mounts.some((m) => m.kind === 'volume' && m.source === name))
  const on = (name: string): Container[] => containers.filter((c) => (specs.get(c.id) ?? specOf(c)).network === name)
  const namesOf = (list: Container[]): string[] => list.map((c) => c.name).sort()
  const listedImages = (): Image[] =>
    [...images]
      .sort((a, b) => (a.tags[0] && b.tags[0] ? a.tags[0].localeCompare(b.tags[0]) : a.tags[0] ? -1 : b.tags[0] ? 1 : b.created - a.created))
      .map((i) => ({ ...i, tags: [...i.tags], containers: namesOf(madeFrom(i)) }))
  const listedVolumes = (): Volume[] =>
    [...volumes].sort((a, b) => a.name.localeCompare(b.name)).map((v) => ({ ...v, containers: namesOf(mounting(v.name)) }))
  const listedNetworks = (): Network[] =>
    [...networks].sort((a, b) => a.name.localeCompare(b.name)).map((n) => ({ ...n, containers: namesOf(on(n.name)) }))
  const reachable = (): void => {
    if (problem !== '') throw new Refused(problem)
  }
  /** A name Docker takes for a volume or a network. */
  const NAME = /^[a-zA-Z0-9][a-zA-Z0-9_.-]+$/
  const short = (id: string): string => id.replace(/^sha256:/, '').slice(0, 12)
  const pretty = (names: string[]): string => (names.length <= 1 ? (names[0] ?? '') : `${names.slice(0, -1).join(', ')} and ${names.at(-1)}`)

  const listedRecipes = (): Recipe[] => [...recipes].sort((a, b) => a.tag.localeCompare(b.tag)).map((r) => ({ ...r }))
  const tagged = (name: string): string => (/:[^/]*$/.test(name) ? name : `${name}:latest`)
  /** The recipe's name and Dockerfile, checked as the backend checks them, in its words. */
  function recipeChecked(tag: string, dockerfile: string, except?: string): string {
    const name = tag.trim()
    if (name === '') throw new Refused('Say what the image is called, like tools:1.')
    if (!/^[a-z0-9][a-z0-9._/-]*(:[\w][\w.-]*)?$/.test(name)) {
      throw new Refused(`${name} is not the name of an image. An image is named like tools, tools:1 or ghcr.io/owner/tools:1.`)
    }
    const full = tagged(name)
    if (recipes.some((r) => r.tag === full && r.id !== except)) throw new Refused(`Another recipe builds ${full} already.`)
    if (dockerfile.trim() === '') throw new Refused('Write the Dockerfile the image is built from.')
    return full
  }
  const recipeBy = (id: string): Recipe => {
    const found = recipes.find((r) => r.id === id)
    if (!found) throw new Refused('There is no such recipe.')
    return found
  }
  /** Build a recipe's image, as the backend does: on a later turn, its lines told, the name moved to the new image. */
  function build(found: Recipe): void {
    if (found.building) return
    found.building = true
    const { id, tag, dockerfile } = found
    const tell = (line: string, status: VosApp.Containers.BuildStatus): void => emit('building', { recipe: id, line, status })
    const run = (): void => {
      const kept = recipes.find((r) => r.id === id)
      tell(`Building ${tag}`, 'running')
      const steps = dockerfile.split('\n').filter((l) => l.trim() !== '' && !l.trim().startsWith('#'))
      steps.forEach((step, i) => tell(`Step ${i + 1}/${steps.length} : ${step.trim()}`, 'running'))
      const failure = problem || buildFailure(dockerfile)
      if (kept) kept.building = false
      if (failure) {
        const said = problem || `The image could not be built: ${failure}`
        if (kept) kept.problem = said
        tell(said, 'failed')
        return
      }
      const built = image([tag], { id: image([`${tag}\n${dockerfile}`]).id })
      images = [...images.map((i) => ({ ...i, tags: i.tags.filter((t) => t !== tag) })).filter((i) => i.id !== built.id), built]
      builtFrom.set(id, `${tag}
${dockerfile}`)
      if (kept) Object.assign(kept, { image: built.id, current: kept.tag === tag && kept.dockerfile === dockerfile, problem: '' })
      tell(`Successfully tagged ${tag}`, 'running')
      tell(`Built ${tag}.`, 'done')
    }
    if (options.holdBuilds) held.push(run)
    else setTimeout(run, pace)
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
    if (!images.some((i) => i.tags.includes(image))) pull(image)
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
        volumes = [...volumes, volume(m.source.trim())]
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
    images: () =>
      answered(() => {
        reachable()
        return listedImages()
      }),
    pullImage: ({ image: name }) =>
      answered(() => {
        reachable()
        const wanted = name.trim()
        if (wanted === '') throw new Refused('Say which image to pull, like nginx:1.27.')
        // what the backend says of a name that is no image's is what it says of a container's
        const wrong = refusal({ ...specOf(container(wanted, wanted)), image: wanted })
        if (wrong) throw new Refused(wrong)
        pull(wanted)
        return listedImages()
      }),
    removeImage: ({ image: id, force }) =>
      answered(() => {
        reachable()
        const found = images.find((i) => i.id === id)
        if (!found) throw new Refused(`No such image: ${id}`)
        const users = madeFrom(found)
        const running = users.find((c) => c.state === 'running')
        if (running) throw new Refused(`conflict: unable to delete ${short(found.id)} (cannot be forced) - image is being used by running container ${short(running.id)}`)
        if (users[0] && !force) throw new Refused(`conflict: unable to delete ${short(found.id)} (must be forced) - image is being used by stopped container ${short(users[0].id)}`)
        images = images.filter((i) => i !== found)
        return listedImages()
      }),
    pruneImages: () =>
      answered(() => {
        reachable()
        const doomed = images.filter((i) => i.tags.length === 0 && madeFrom(i).length === 0)
        images = images.filter((i) => !doomed.includes(i))
        return { images: listedImages(), removed: doomed.length, space: doomed.reduce((sum, i) => sum + i.size, 0) }
      }),
    volumes: () =>
      answered(() => {
        reachable()
        return listedVolumes()
      }),
    createVolume: ({ name, driver, labels }) =>
      answered(() => {
        reachable()
        if (name !== '' && !NAME.test(name)) throw new Refused("A volume's name starts with a letter or a digit, and has only letters, digits, _, . and -.")
        if (volumes.some((v) => v.name === name)) throw new Refused(`There is a volume named ${name} already.`)
        if (labels.some((l) => l.name.trim() === '')) throw new Refused(`${labels.length === 1 ? 'Label' : 'A label'} has no name.`)
        volumes = [...volumes, volume(name || `${'a'.repeat(56)}${String(volumes.length).padStart(8, '0')}`, { driver: driver || 'local' })]
        return listedVolumes()
      }),
    removeVolume: ({ volume: name }) =>
      answered(() => {
        reachable()
        const users = namesOf(mounting(name))
        if (users.length > 0) throw new Refused(`The volume ${name} is used by ${pretty(users)}: Docker removes a volume only once no container uses it, running or not.`)
        if (!volumes.some((v) => v.name === name)) throw new Refused(`get ${name}: no such volume`)
        volumes = volumes.filter((v) => v.name !== name)
        return listedVolumes()
      }),
    pruneVolumes: () =>
      answered(() => {
        reachable()
        const doomed = volumes.filter((v) => mounting(v.name).length === 0)
        volumes = volumes.filter((v) => !doomed.includes(v))
        return { volumes: listedVolumes(), removed: doomed.map((v) => v.name).sort(), space: doomed.length * 24_000_000 }
      }),
    networks: () =>
      answered(() => {
        reachable()
        return listedNetworks()
      }),
    createNetwork: ({ name, driver, subnet, gateway, internal, attachable }) =>
      answered(() => {
        reachable()
        if (name.trim() === '') throw new Refused('Say what the network is called.')
        if (!NAME.test(name.trim())) throw new Refused("A network's name starts with a letter or a digit, and has only letters, digits, _, . and -.")
        if (subnet.trim() !== '' && !/^\d+\.\d+\.\d+\.\d+\/\d+$/.test(subnet.trim())) {
          throw new Refused(`${subnet.trim()} is not a subnet. A subnet is an address and the size of its prefix, like 172.30.0.0/16.`)
        }
        if (gateway.trim() !== '' && subnet.trim() === '') throw new Refused("A gateway is an address in the network's subnet: say the subnet too.")
        if (networks.some((n) => n.name === name.trim())) throw new Refused(`network with name ${name.trim()} already exists`)
        if (driver === 'overlay') {
          throw new Refused('This node is not a swarm manager. Use "docker swarm init" or "docker swarm join" to connect this node to swarm and try again.')
        }
        const next = 18 + networks.length
        networks = [
          ...networks,
          network(name.trim(), {
            driver: driver || 'bridge',
            subnet: subnet.trim() || `172.${next}.0.0/16`,
            gateway: gateway.trim() || (subnet.trim() ? subnet.trim().replace(/\.\d+\/\d+$/, '.1') : `172.${next}.0.1`),
            internal,
            attachable
          })
        ]
        return listedNetworks()
      }),
    removeNetwork: ({ network: name }) =>
      answered(() => {
        reachable()
        const found = networks.find((n) => n.name === name)
        if (!found) throw new Refused(`network ${name} not found`)
        if (found.predefined) throw new Refused(`${name} is Docker's own network: it cannot be removed.`)
        const running = namesOf(on(name).filter((c) => c.state === 'running'))
        if (running.length > 0) {
          throw new Refused(`${pretty(running)} ${running.length === 1 ? 'runs' : 'run'} on the network ${name}: Docker removes a network only once no running container is on it.`)
        }
        networks = networks.filter((n) => n !== found)
        return listedNetworks()
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
      }),
    recipes: () => answered(listedRecipes),
    addRecipe: ({ tag, dockerfile }) =>
      answered(() => {
        const full = recipeChecked(tag, dockerfile)
        recipes = [...recipes, recipe(full, { id: `recipe-${recipes.length + 1}-${full}`, dockerfile })]
        return listedRecipes()
      }),
    updateRecipe: ({ recipe: id, tag, dockerfile }) =>
      answered(() => {
        const found = recipeBy(id)
        const full = recipeChecked(tag, dockerfile, id)
        Object.assign(found, { tag: full, dockerfile, current: found.image !== '' && builtFrom.get(id) === `${full}
${dockerfile}` })
        return listedRecipes()
      }),
    removeRecipe: ({ recipe: id }) =>
      answered(() => {
        recipeBy(id)
        recipes = recipes.filter((r) => r.id !== id)
        return listedRecipes()
      }),
    build: ({ recipe: id }) =>
      answered(() => {
        build(recipeBy(id))
        return listedRecipes()
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
    },
    finish() {
      const runs = held
      held = []
      runs.forEach((run) => run())
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
      images: refuse,
      pullImage: refuse,
      removeImage: refuse,
      pruneImages: refuse,
      volumes: refuse,
      createVolume: refuse,
      removeVolume: refuse,
      pruneVolumes: refuse,
      networks: refuse,
      createNetwork: refuse,
      removeNetwork: refuse,
      startStack: refuse,
      stopStack: refuse,
      restartStack: refuse,
      removeStack: refuse,
      details: refuse,
      logs: refuse,
      recipes: refuse,
      addRecipe: refuse,
      updateRecipe: refuse,
      removeRecipe: refuse,
      build: refuse
    },
    events: { on: () => () => {} },
    emit: () => {},
    outside: () => {},
    reach: () => {},
    write: () => {},
    finish: () => {}
  }
}
