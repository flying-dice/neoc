/**
 * A stand-in for the DNS app's backend, for stories and unit tests: the configuration and a container kept in the
 * page, with no host and no Docker. It checks a name and an address as the backend does, makes its container again
 * on every change while the server is on, and writes a line of log for each.
 */
type Logs = VosApp.Dns.Logs
type Overview = VosApp.Dns.Overview
type DnsRecord = VosApp.Dns.Record
type Server = VosApp.Dns.Server
type ServerState = VosApp.Dns.ServerState
type Settings = VosApp.Dns.Settings
import type { DnsEvents, DnsRpc } from './dns.svelte'
import { isAddress, nameProblem } from './validate'

export interface FakeBackend {
  readonly rpc: DnsRpc
  readonly events: DnsEvents
  /** Send an event to the window, as the host does (`host:reconnected`) or the backend's watcher (`changed`). */
  emit(name: string, data?: unknown): void
  /** The container changes behind the app's back, as a terminal would change it: the windows are told. */
  outside(state: ServerState): void
  /** A record is added elsewhere (another window, an agent): the windows are told. */
  elsewhere(name: string, address: string): void
  /** Docker goes away, or comes back. */
  reach(reachable: boolean): void
  /** The server writes a line. */
  write(line: string): void
  /** How many times the container was made. */
  made(): number
}

export interface FakeOptions {
  /** The records that are there (default: `RECORDS`). */
  records?: readonly DnsRecord[]
  settings?: Settings
  /** The server is wanted on, and its container runs (default: it is). */
  enabled?: boolean
  /** Docker cannot be reached, for this reason. */
  problem?: string
  /** The address that is chosen (default: this machine's own, empty). */
  host?: string
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
  /** Docker refuses to start the container, with these words, as it does when the port is taken. */
  refusing?: string
}

/** A failure the backend answers a call with (`ctx:fail`), as the page's client rejects with it. */
class Refused extends Error {
  readonly code = -32010
}

export const IMAGE = '4km3/dnsmasq:2.90-r3-alpine-3.22'
export const PORT_TAKEN = 'driver failed programming external connectivity on endpoint vos-dns: Bind for 0.0.0.0:53 failed: port is already allocated'

/** A record, for a story's or a test's starting point. */
export const record = (id: number, name: string, address: string, subdomains = false): DnsRecord => ({ id, name, address, subdomains })

/** A home network: a NAS with both kinds of address, a printer, and everything under one name. */
export const RECORDS: readonly DnsRecord[] = [
  record(1, 'nas.home', '192.168.1.10'),
  record(2, 'nas.home', 'fd00::10'),
  record(3, 'printer.home', '192.168.1.31'),
  record(4, 'apps.home', '192.168.1.20', true)
]

export const SETTINGS: Settings = { port: 53, upstreams: ['1.1.1.1', '1.0.0.1'], logQueries: false }

const ADDRESS = /^(unix:\/\/\/|tcp:\/\/[^/]|https?:\/\/[^/])/
/** An IP address as the backend keeps it; refused, in the backend's words, when it is not one. */
function addressOf(raw: string): string {
  const text = raw.trim().toLowerCase()
  if (isAddress(text)) return text
  throw new Refused(`${text === '' ? 'That' : text} is not an IP address, like 192.168.1.10 or fd00::10.`)
}

/** A name as the backend keeps it; refused, in the backend's words, when it is not one. */
function nameOf(raw: string): string {
  const text = raw.trim().toLowerCase().replace(/\.$/, '')
  if (text === '') throw new Refused('The name is blank.')
  if (nameProblem(text) !== null) {
    throw new Refused(
      'A name is letters, digits and hyphens, in parts of up to 63 joined by dots, like nas.home. A part does not start or end with a hyphen.'
    )
  }
  return text
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  let records: DnsRecord[] = (options.records ?? RECORDS).map((r) => ({ ...r }))
  let settings: Settings = { ...(options.settings ?? SETTINGS) }
  let enabled = options.enabled ?? true
  let problem = options.problem ?? ''
  let host = options.host ?? ''
  const pace = options.pace ?? 0
  // the container: how it is doing, and whether it was made from the configuration as it stands
  let state: ServerState = enabled && options.refusing === undefined ? 'running' : enabled ? 'stopped' : 'absent'
  let current = enabled
  let refused = enabled ? (options.refusing ?? '') : ''
  let made = 0
  let next = Math.max(0, ...records.map((r) => r.id)) + 1
  let written: string[] = state === 'running' ? ['dnsmasq: started, version 2.90 cachesize 1000', 'dnsmasq: using nameserver 1.1.1.1#53'] : []
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))
  const answered = <T>(answer: () => T): Promise<T> => new Promise((resolve, reject) => setTimeout(() => {
    try {
      resolve(answer())
    } catch (e) {
      reject(e as Error)
    }
  }, pace))

  const server = (): Server => {
    const reachable = problem === ''
    const there = reachable ? state : 'absent'
    return {
      enabled,
      state: there,
      status: there === 'running' ? 'Up 2 hours' : there === 'stopped' ? 'Created' : there === 'restarting' ? 'Restarting (2) 3 seconds ago' : '',
      current: there !== 'absent' && current,
      image: IMAGE,
      problem: refused
    }
  }
  const overview = (): Overview => ({
    engine: { reachable: problem === '', version: problem === '' ? '27.3.1' : '', host, problem },
    server: server(),
    settings: { ...settings, upstreams: [...settings.upstreams] },
    records: [...records].sort((a, b) => a.name.localeCompare(b.name) || a.address.localeCompare(b.address)).map((r) => ({ ...r }))
  })

  /** Make the container from the configuration, and start it: what the backend's sidecar does. */
  const apply = (): void => {
    if (problem !== '') {
      refused = problem
      return
    }
    made += 1
    current = true
    refused = options.refusing ?? ''
    state = refused === '' ? 'running' : 'stopped'
    written = refused === '' ? [`dnsmasq: started, version 2.90 cachesize 1000`, `dnsmasq: read ${records.length} names`] : []
  }
  /** The configuration changed: a server that is on is made again. */
  const reconfigured = (): Overview => {
    if (enabled) apply()
    return overview()
  }

  const rpc: DnsRpc = {
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
    setEnabled: ({ enabled: wanted }) =>
      answered(() => {
        enabled = wanted
        if (wanted) {
          apply()
        } else {
          state = 'absent'
          current = false
          refused = ''
          written = []
        }
        return overview()
      }),
    apply: () => answered(reconfigured),
    saveSettings: ({ port, upstreams, logQueries }) =>
      answered(() => {
        settings = { port, upstreams: [...new Set(upstreams.map(addressOf))], logQueries }
        return reconfigured()
      }),
    addRecord: ({ name, address, subdomains }) =>
      answered(() => {
        const added = record(next, nameOf(name), addressOf(address), subdomains)
        if (records.some((r) => r.name === added.name && r.address === added.address)) {
          throw new Refused(`${added.name} has the address ${added.address} already.`)
        }
        next += 1
        records = [...records, added]
        return reconfigured()
      }),
    removeRecord: ({ record: id }) =>
      answered(() => {
        if (!records.some((r) => r.id === id)) throw new Refused('There is no such record.')
        records = records.filter((r) => r.id !== id)
        return reconfigured()
      }),
    logs: (params) =>
      answered((): Logs => {
        if (problem !== '') throw new Refused(problem)
        if (state === 'absent') throw new Refused('The server has no container: it is off.')
        const shown = written.slice(-(params?.tail ?? 200))
        return { text: shown.map((line) => `${line}\n`).join(''), truncated: shown.length < written.length }
      })
  }

  const events: DnsEvents = {
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
    outside(changed) {
      state = changed
      emit('changed')
    },
    elsewhere(name, address) {
      records = [...records, record(next, name, address)]
      next += 1
      if (enabled) apply()
      emit('changed')
    },
    reach(reachable) {
      problem = reachable ? '' : 'Docker is not running on this machine, or cannot be reached.'
      emit('changed')
    },
    write(line) {
      written = [...written, line]
    },
    made: () => made
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: {
      overview: refuse,
      connect: refuse,
      setEnabled: refuse,
      apply: refuse,
      saveSettings: refuse,
      addRecord: refuse,
      removeRecord: refuse,
      logs: refuse
    },
    events: { on: () => () => {} },
    emit: () => {},
    outside: () => {},
    elsewhere: () => {},
    reach: () => {},
    write: () => {},
    made: () => 0
  }
}
