/**
 * A stand-in for the Pages app's backend, for stories and unit tests: the configuration and a container kept in the
 * page, with no host and no Docker. It checks a site as the backend does, makes its container again on every change
 * that makes a difference, and writes a line of log for each.
 */
type Logs = VosApp.Pages.Logs
type Overview = VosApp.Pages.Overview
type Server = VosApp.Pages.Server
type ServerState = VosApp.Pages.ServerState
type Settings = VosApp.Pages.Settings
type Site = VosApp.Pages.Site
import type { PagesHost, PagesRpc } from './pages.svelte'

export interface FakeBackend {
  readonly rpc: PagesRpc
  readonly host: PagesHost
  /** Send an event to the window, as the host does (`host:reconnected`) or the backend's watcher (`changed`). */
  emit(name: string, data?: unknown): void
  /** The container changes behind the app's back, as a terminal would change it: the windows are told. */
  outside(state: ServerState): void
  /** A site is added elsewhere (another window, an agent): the windows are told. */
  elsewhere(name: string, folder: string, port: number): void
  /** Docker goes away, or comes back. */
  reach(reachable: boolean): void
  /** The server writes a line. */
  write(line: string): void
  /** How many times the container was made. */
  made(): number
  /** The addresses that were opened outside the app. */
  opened(): readonly string[]
}

export interface FakeOptions {
  /** The sites that are there (default: `SITES`). */
  sites?: readonly Site[]
  settings?: Settings
  /** Docker cannot be reached, for this reason. */
  problem?: string
  /** The address that is chosen (default: this machine's own, empty). */
  host?: string
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
  /** Docker refuses to start the container, with these words, as it does when a port is taken. */
  refusing?: string
  /** The folders that are not there: a site of one is on and not served. */
  missing?: readonly string[]
}

/** A failure the backend answers a call with (`ctx:fail`). */
class Refused extends Error {}

export const IMAGE = 'nginx:1.30.0-alpine'
export const PORT_TAKEN =
  'driver failed programming external connectivity on endpoint vos-pages: Bind for 0.0.0.0:8081 failed: port is already allocated'
export const NO_FOLDER = 'The folder is not there.'

/** A site, for a story's or a test's starting point. */
export const site = (id: number, name: string, folder: string, port: number, hostnames: readonly string[] = [], enabled = true): Site => ({
  id,
  name,
  folder,
  port,
  hostnames: [...hostnames],
  enabled,
  problem: ''
})

/** A home server: a blog with a port and a name, a family wiki with a name only, and an old site that is off. */
export const SITES: readonly Site[] = [
  site(1, 'Blog', '/Users/me/Sites/blog', 8081, ['blog.home']),
  site(2, 'Family Wiki', '/Users/me/Sites/wiki', 0, ['wiki.home', 'www.wiki.home']),
  site(3, 'Old Portfolio', '/Users/me/Sites/portfolio-2019', 8090, [], false)
]

export const SETTINGS: Settings = { port: 80 }

const ADDRESS = /^(unix:\/\/\/|tcp:\/\/[^/]|https?:\/\/[^/])/
const LABEL = /^[a-z0-9]([a-z0-9-]{0,61}[a-z0-9])?$/

/** A hostname as the backend keeps it; refused when it is not one. */
function hostnameOf(raw: string): string {
  const text = raw.trim().toLowerCase().replace(/\.$/, '')
  if (text === '') throw new Refused('A hostname is blank.')
  if (!text.split('.').every((label) => LABEL.test(label))) {
    throw new Refused(
      `${text} is not a hostname: letters, digits and hyphens, in parts of up to 63 joined by dots, like blog.home. A part does not start or end with a hyphen.`
    )
  }
  return text
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let sites: Site[] = (options.sites ?? SITES).map((s) => ({ ...s, hostnames: [...s.hostnames] }))
  let settings: Settings = { ...(options.settings ?? SETTINGS) }
  let problem = options.problem ?? ''
  let host = options.host ?? ''
  const pace = options.pace ?? 0
  const missing = new Set(options.missing ?? [])

  const withProblems = (): Site[] => sites.map((s) => ({ ...s, hostnames: [...s.hostnames], problem: s.enabled && missing.has(s.folder) ? NO_FOLDER : '' }))
  const served = (): Site[] => withProblems().filter((s) => s.enabled && s.problem === '')
  /** What the container is made from, as one word. */
  const signature = (): string => JSON.stringify([settings.port, served().map((s) => [s.id, s.folder, s.port, s.hostnames])])

  // the container: how it is doing, and what it was made from
  let state: ServerState = served().length === 0 ? 'absent' : options.refusing === undefined ? 'running' : 'stopped'
  let madeFrom = state === 'absent' ? '' : signature()
  let refused = state === 'stopped' ? (options.refusing ?? '') : ''
  let made = 0
  let next = Math.max(0, ...sites.map((s) => s.id)) + 1
  let written: string[] = state === 'running' ? ['nginx/1.30.0', 'start worker processes'] : []
  const opened: string[] = []
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))
  const answered = <T>(answer: () => T): Promise<T> =>
    new Promise((resolve, reject) =>
      setTimeout(() => {
        try {
          resolve(answer())
        } catch (e) {
          reject(e as Error)
        }
      }, pace)
    )

  const server = (): Server => {
    const reachable = problem === ''
    const there = reachable ? state : 'absent'
    return {
      wanted: sites.some((s) => s.enabled),
      state: there,
      status: there === 'running' ? 'Up 2 hours' : there === 'stopped' ? 'Created' : there === 'restarting' ? 'Restarting (1) 3 seconds ago' : '',
      current: there !== 'absent' && madeFrom === signature(),
      image: IMAGE,
      problem: refused
    }
  }
  const overview = (): Overview => ({
    engine: { reachable: problem === '', version: problem === '' ? '27.3.1' : '', host, problem },
    server: server(),
    settings: { ...settings },
    sites: withProblems().sort((a, b) => a.name.toLowerCase().localeCompare(b.name.toLowerCase()) || a.id - b.id)
  })

  /** Have the container be what the configuration says: what the backend's sidecar does. */
  const apply = (always: boolean): void => {
    if (problem !== '') {
      refused = problem
      return
    }
    if (served().length === 0) {
      state = 'absent'
      madeFrom = ''
      refused = ''
      written = []
      return
    }
    if (!always && state === 'running' && madeFrom === signature()) return
    made += 1
    madeFrom = signature()
    refused = options.refusing ?? ''
    state = refused === '' ? 'running' : 'stopped'
    written = refused === '' ? ['nginx/1.30.0', `start worker processes for ${served().length} sites`] : []
  }
  const reconfigured = (): Overview => {
    apply(false)
    return overview()
  }

  /** What was given for a site, as it is kept; refused when it cannot be. Close to the backend's check. */
  const checked = (given: { name: string; folder: string; port: number; hostnames: string[] }, except: number | null): Omit<Site, 'id' | 'enabled' | 'problem'> => {
    const name = given.name.trim()
    if (name === '') throw new Refused('The name is blank.')
    let folder = given.folder.trim()
    if (folder === '') throw new Refused('The folder is blank.')
    if (!folder.startsWith('/')) throw new Refused('The folder must be a full path, like /Users/me/Sites/blog.')
    folder = folder.replace(/\/+$/, '')
    if (folder === '') throw new Refused('The whole disk cannot be a site: choose a folder in it.')
    const hostnames = [...new Set(given.hostnames.filter((raw) => raw.trim() !== '').map(hostnameOf))].sort()
    if (given.port === 0 && hostnames.length === 0) {
      throw new Refused('A site needs a port, a hostname, or both: there is no other way to reach it.')
    }
    if (given.port !== 0 && given.port === settings.port) {
      throw new Refused(`Port ${given.port} is the one the sites with hostnames share. Choose another, or change that one.`)
    }
    for (const other of sites) {
      if (other.id === except) continue
      if (given.port !== 0 && other.port === given.port) throw new Refused(`Port ${given.port} is the port of ${other.name}.`)
      const taken = hostnames.find((hostname) => other.hostnames.includes(hostname))
      if (taken !== undefined) throw new Refused(`${taken} is a hostname of ${other.name}.`)
    }
    return { name, folder, port: given.port, hostnames }
  }
  const found = (id: number): Site => {
    const one = sites.find((s) => s.id === id)
    if (!one) throw new Refused('There is no such site.')
    return one
  }

  const rpc: PagesRpc = {
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
    apply: () =>
      answered(() => {
        apply(true)
        return overview()
      }),
    saveSettings: ({ port }) =>
      answered(() => {
        const owner = sites.find((s) => s.port === port)
        if (owner) throw new Refused(`Port ${port} is the own port of ${owner.name}.`)
        settings = { port }
        return reconfigured()
      }),
    addSite: (given) =>
      answered(() => {
        sites = [...sites, { id: next, ...checked(given, null), enabled: true, problem: '' }]
        next += 1
        return reconfigured()
      }),
    updateSite: ({ site: id, ...given }) =>
      answered(() => {
        const one = found(id)
        const changed = checked(given, id)
        sites = sites.map((s) => (s === one ? { ...s, ...changed } : s))
        return reconfigured()
      }),
    setSiteEnabled: ({ site: id, enabled }) =>
      answered(() => {
        const one = found(id)
        sites = sites.map((s) => (s === one ? { ...s, enabled } : s))
        return reconfigured()
      }),
    removeSite: ({ site: id }) =>
      answered(() => {
        const one = found(id)
        sites = sites.filter((s) => s !== one)
        return reconfigured()
      }),
    logs: (params) =>
      answered((): Logs => {
        if (problem !== '') throw new Refused(problem)
        if (state === 'absent') throw new Refused('The server has no container: no site is on.')
        const shown = written.slice(-(params?.tail ?? 200))
        return { text: shown.map((line) => `${line}\n`).join(''), truncated: shown.length < written.length }
      })
  }

  const hostOf: PagesHost = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    },
    open_link(url) {
      opened.push(url)
      return Promise.resolve()
    }
  }

  return {
    rpc,
    host: hostOf,
    emit,
    outside(changed) {
      state = changed
      if (changed === 'absent') madeFrom = ''
      emit('changed')
    },
    elsewhere(name, folder, port) {
      sites = [...sites, site(next, name, folder, port)]
      next += 1
      apply(false)
      emit('changed')
    },
    reach(reachable) {
      problem = reachable ? '' : 'Docker is not running on this machine, or cannot be reached.'
      emit('changed')
    },
    write(line) {
      written = [...written, line]
    },
    made: () => made,
    opened: () => opened
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      overview: refuse,
      connect: refuse,
      apply: refuse,
      saveSettings: refuse,
      addSite: refuse,
      updateSite: refuse,
      setSiteEnabled: refuse,
      removeSite: refuse,
      logs: refuse
    },
    host: { on: () => () => {}, open_link: refuse },
    emit: () => {},
    outside: () => {},
    elsewhere: () => {},
    reach: () => {},
    write: () => {},
    made: () => 0,
    opened: () => []
  }
}
