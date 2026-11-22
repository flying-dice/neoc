/**
 * A stand-in for the Containers app's backend, for stories and unit tests: a Docker kept in the page, with no host and
 * no daemon. It has containers that start, stop and go away as the real ones do, stacks worked out from them as the
 * backend works them out, and a line of log for everything that happens to a container.
 */
import type { Container, Details, Overview, Stack, State } from 'app:rpc'
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
    remove: onContainer('remove'),
    startStack: onStack('start'),
    stopStack: onStack('stop'),
    restartStack: onStack('restart'),
    removeStack: onStack('remove'),
    details: ({ container: id }) =>
      answered((): Details => {
        const found = find(id)
        return {
          container: id,
          command: found.image.startsWith('postgres') ? 'docker-entrypoint.sh postgres' : '/init',
          restart: found.stack === '' ? 'no' : 'unless-stopped',
          health: found.image.startsWith('postgres') && found.state === 'running' ? 'healthy' : '',
          mounts: [`${found.name}-data -> /data`],
          networks: [found.stack === '' ? 'bridge' : `${found.stack}_default`],
          environment: ['PATH', 'TZ']
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
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
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
