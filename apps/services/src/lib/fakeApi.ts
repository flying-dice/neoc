/**
 * A stand-in for the Services app's backend, for stories and unit tests: a service manager kept in the page, with no
 * host and no operating system. Its services start, stop and are removed as a real manager's do, it refuses a change of
 * a service it keeps for another user, it cannot start one whose program exits at once, and it can be changed behind
 * the app's back, as a terminal would change the real one.
 */
type Manager = VosApp.Services.Manager
type Outcome = VosApp.Services.Outcome
type Overview = VosApp.Services.Overview
type Restart = VosApp.Services.Restart
type Service = VosApp.Services.Service
type Status = VosApp.Services.Status
import type { ServicesEvents, ServicesRpc } from './services.svelte'

export interface FakeBackend {
  readonly rpc: ServicesRpc
  readonly events: ServicesEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string): void
  /** The manager is changed by another program: the next overview shows it, nothing is told. */
  outside(change: (services: Service[]) => Service[]): void
  /** The calls the backend answered, in order, by method name. */
  readonly calls: string[]
}

export interface FakeOptions {
  services?: readonly Service[]
  /** The manager cannot be used, for this reason. */
  problem?: string
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
  /** The labels the system refuses to change, as the services of another user. */
  locked?: readonly string[]
  /** The labels that cannot be started. */
  broken?: readonly string[]
  /** What the manager keeps and says (default: everything, like the mock). */
  manager?: Partial<Manager>
}

export const SYSTEMD: Manager = {
  available: true,
  problem: '',
  kind: 'systemd',
  level: 'system',
  installOptions: ['args', 'working_directory', 'environment', 'autostart', 'restart', 'restart_delay_secs', 'user', 'description'],
  configurable: ['autostart', 'description', 'program', 'args', 'working_directory', 'environment', 'restart', 'restart_delay_secs'],
  reported: ['description', 'state', 'state_detail', 'autostart', 'program', 'args', 'working_directory', 'environment', 'restart', 'restart_delay_secs', 'user', 'path']
}

/** A launchd-like manager: no description, nothing said of the user's files. */
export const LAUNCHD: Manager = {
  ...SYSTEMD,
  kind: 'launchd',
  installOptions: ['args', 'working_directory', 'environment', 'autostart', 'restart', 'restart_delay_secs', 'user'],
  configurable: ['autostart', 'program', 'args', 'working_directory', 'environment', 'restart', 'restart_delay_secs'],
  reported: ['state', 'state_detail', 'autostart', 'program', 'args', 'working_directory', 'environment', 'restart', 'restart_delay_secs', 'user', 'path']
}

export const service = (label: string, extra: Partial<Service> = {}): Service => ({
  label,
  description: `The ${label} service`,
  state: 'stopped',
  autostart: false,
  program: `/usr/sbin/${label}`,
  args: [],
  restart: 'on_failure',
  restartDelaySecs: 5,
  path: `/usr/lib/systemd/system/${label}.service`,
  ...extra
})

/** A machine with services of its own, a failed one, one that starts by itself, and one of another user. */
export const SERVICES: readonly Service[] = [
  service('cups', { description: 'A printing service', state: 'stopped', autostart: false }),
  service('nginx', { description: 'A web server', state: 'running', autostart: true, args: ['-g', 'daemon off;'], workingDirectory: '/srv/www', environment: [{ name: 'LANG', value: 'C' }] }),
  service('postgres', { description: 'The database', state: 'running', autostart: true, restart: 'always', user: 'postgres' }),
  service('backup', { description: 'The nightly backup', state: 'failed', stateDetail: 'exit-code', autostart: true }),
  service('systemd-logind', { description: 'Login management', state: 'running', autostart: undefined })
]

export const NO_MANAGER = 'This machine has no service manager the app can use: systemd is not running on this machine.'

/** The backend answered no, in its own words. */
const denied = 'The operating system did not allow this. Changing a service needs administrator rights, which this app does not raise on its own.'

export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let services: Service[] = (options.services ?? SERVICES).map((s) => ({ ...s }))
  const manager: Manager = options.problem
    ? { ...SYSTEMD, available: false, problem: options.problem, kind: '', installOptions: [], configurable: [], reported: [] }
    : { ...SYSTEMD, ...options.manager }
  const locked = new Set(options.locked ?? ['systemd-logind'])
  const broken = new Set(options.broken ?? ['backup'])
  const listeners = new Map<string, Set<() => void>>()
  const calls: string[] = []

  const overview = (): Overview => ({ manager, services: manager.available ? services.map((s) => ({ ...s })).sort((a, b) => a.label.localeCompare(b.label)) : [] })
  const outcome = (status: Status, message = ''): Outcome => ({ status, message, overview: overview() })
  const later = async <T>(call: () => T): Promise<T> => {
    await new Promise((resolve) => setTimeout(resolve, options.pace ?? 0))
    return call()
  }
  const find = (label: string): Service | undefined => services.find((s) => s.label === label)
  const change = (label: string, apply: (s: Service) => Outcome | void): Outcome => {
    const found = find(label)
    if (!found) return outcome('missing', 'This service is not there any more.')
    if (locked.has(label)) return outcome('denied', denied)
    return apply(found) ?? outcome('done')
  }
  const control = (name: string, label: string, apply: (s: Service) => Outcome | void): Promise<Outcome> => {
    calls.push(`${name} ${label}`)
    return later(() => change(label, apply))
  }
  const start = (s: Service): Outcome | void => {
    if (broken.has(s.label)) {
      s.state = 'failed'
      return outcome('failed', `The service manager could not start: ${s.label} exited with code 1 at once`)
    }
    s.state = 'running'
    delete s.stateDetail
  }

  const rpc: ServicesRpc = {
    overview: () => {
      calls.push('overview')
      return later(overview)
    },
    start: ({ label }) => control('start', label, start),
    stop: ({ label }) =>
      control('stop', label, (s) => {
        s.state = 'stopped'
        delete s.stateDetail
      }),
    restart: ({ label }) =>
      control('restart', label, (s) => {
        s.state = 'stopped'
        return start(s)
      }),
    configure: ({ label, autostart, description, restart }) =>
      control('configure', label, (s) => {
        for (const [field] of Object.entries({ autostart, description, restart })) {
          const need = field === 'restart' ? 'restart' : field
          if ({ autostart, description, restart }[field as 'autostart'] !== undefined && !manager.configurable.includes(need)) {
            return outcome('unsupported', `This machine's service manager cannot do that: it does not keep the option "${need}"`)
          }
        }
        if (autostart !== undefined) s.autostart = autostart
        if (description !== undefined) s.description = description
        if (restart !== undefined) s.restart = restart as Restart
      }),
    register: ({ label, program, args, description, workingDirectory, environment, autostart, restart, restartDelaySecs }) => {
      calls.push(`register ${label}`)
      return later(() => {
        if (!/^[A-Za-z0-9_][A-Za-z0-9_.@:-]{0,199}$/.test(label)) return outcome('invalid', `label "${label}" must be 1 to 200 letters, digits or \`_ . - @ :\`, starting with a letter, a digit or \`_\``)
        if (!program.startsWith('/')) return outcome('invalid', `option "program" must be an absolute path, not "${program}"`)
        if (find(label)) return outcome('invalid', `A service called ${label} is there already.`)
        if (description !== undefined && !manager.installOptions.includes('description')) return outcome('unsupported', 'This machine\'s service manager cannot do that: it does not keep the option "description"')
        if (environment !== undefined && !manager.installOptions.includes('environment')) return outcome('unsupported', 'This machine\'s service manager cannot do that: it does not keep the option "environment"')
        services.push(service(label, { program, args: args ?? [], description, workingDirectory, environment, autostart: autostart ?? false, restart, restartDelaySecs }))
        return outcome('done')
      })
    },
    unregister: ({ label }) => control('unregister', label, () => void (services = services.filter((s) => s.label !== label)))
  }
  const events: ServicesEvents = {
    on: (name, handler) => {
      const set = listeners.get(name) ?? new Set<() => void>()
      listeners.set(name, set)
      const listener = (): void => void handler()
      set.add(listener)
      return () => void set.delete(listener)
    }
  }
  return {
    rpc,
    events,
    calls,
    emit: (name) => listeners.get(name)?.forEach((l) => l()),
    outside: (apply) => void (services = apply(services))
  }
}

/** A backend that cannot be reached at all: every call fails the way the host's does. */
export function createFailingBackend(message: string): FakeBackend {
  const fail = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: { overview: fail, start: fail, stop: fail, restart: fail, configure: fail, register: fail, unregister: fail },
    events: { on: () => () => {} },
    calls: [],
    emit: () => {},
    outside: () => {}
  }
}
