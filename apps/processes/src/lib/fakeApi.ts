/**
 * A stand-in for the Processes app's backend, for stories and unit tests: a machine kept in the page, with no host and
 * no operating system. It follows the backend's rules (server/main.luau): a rate has no value until the second reading,
 * a process is chosen before it can be ended, only the process that was chosen is ever signalled (a PID that was
 * reused is another process), asking it to quit is not forcing it, and the system may refuse.
 */
import type { Cpu, Detail, Disk, Machine, Memory, Network, Overview, ProcessRef, ProcessRow, Snapshot, Swap } from 'app:rpc'
import type { ProcessesEvents, ProcessesRpc } from './processes.svelte'

/** What the fake machine knows of one process, beyond what a row shows. */
export interface Program extends ProcessRef {
  name: string
  user?: string
  cpu?: number
  memory: number
  status?: string
  virtualMemory?: number
  command?: string[]
  exe?: string
  parent?: number
  /** It handles the request to quit and carries on: only a forced stop ends it. */
  stubborn?: boolean
  /** The system refuses to let this window end it (it belongs to another user). */
  protected?: boolean
  /** It is the program the app runs in. */
  own?: boolean
}

export interface FakeBackend {
  readonly rpc: ProcessesRpc
  readonly events: ProcessesEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string, data?: unknown): void
  /** The process exits on its own. */
  quit(pid: number): void
  /** The process exits and another one starts with its PID. */
  reuse(pid: number, program: Program): void
  /** A process starts. */
  start(program: Program): void
  /** The signals sent, in order: `terminate 812` or `kill 812`. */
  signals(): readonly string[]
  /** The processes the backend holds a reference to. */
  held(): readonly ProcessRef[]
  /** How many readings were taken. */
  readings(): number
  /** The backend loses what it held (it started afresh). */
  restart(): void
}

export interface FakeOptions {
  /** The processes (default: `PROGRAMS`). */
  programs?: readonly Program[]
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
  /** This system cannot ask a program to quit, as Windows (default: it can). */
  cannotTerminate?: boolean
  /** The machine (default: `MACHINE`). */
  machine?: Partial<{ cpu: Cpu; memory: Memory; swap: Swap; disks: Disk[]; networks: Network[]; machine: Machine }>
}

/** A failure the backend answers a call with (`ctx:fail`), as the page's client rejects with it. */
class Refused extends Error {
  readonly code = -32010
}

const BASE = 1_700_000_000
const GB = 1024 ** 3
const MB = 1024 ** 2

/** A process, for a story's or a test's starting point. */
export const program = (pid: number, name: string, extra: Partial<Program> = {}): Program => ({
  pid,
  startTime: BASE + pid,
  name,
  user: 'vos',
  memory: 50 * MB,
  status: 'sleep',
  ...extra
})

/** What a home lab runs. */
export const PROGRAMS: readonly Program[] = [
  program(1, 'systemd', { user: 'root', cpu: 0.1, memory: 12 * MB, command: ['/sbin/init'], exe: '/usr/lib/systemd/systemd', parent: 0, protected: true }),
  program(391, 'systemd-journald', { user: 'root', cpu: 0.2, memory: 62 * MB, parent: 1, protected: true }),
  program(640, 'dockerd', { user: 'root', cpu: 4.8, memory: 188 * MB, parent: 1, protected: true }),
  program(702, 'sshd', { user: 'root', cpu: 0.4, memory: 14 * MB, parent: 1, protected: true }),
  program(812, 'postgres', {
    user: 'postgres',
    cpu: 6.1,
    memory: 412 * MB,
    virtualMemory: 2 * GB,
    status: 'run',
    parent: 1,
    exe: '/usr/lib/postgresql/16/bin/postgres',
    command: ['/usr/lib/postgresql/16/bin/postgres', '-D', '/var/lib/postgresql/16/main', '-c', 'config_file=/etc/postgresql/16/main/postgresql.conf']
  }),
  program(841, 'postgres: checkpointer', { user: 'postgres', cpu: 0, memory: 20 * MB, parent: 812 }),
  program(842, 'postgres: background writer', { user: 'postgres', cpu: 0, memory: 18 * MB, parent: 812 }),
  program(1204, 'vos-server', { cpu: 12.6, memory: 1.2 * GB, status: 'run', own: true }),
  program(2310, 'luau-runtime', { cpu: 38.2, memory: 612 * MB, status: 'run', parent: 1204 }),
  program(2874, 'node', { cpu: 2.2, memory: 540 * MB }),
  program(3051, 'rust-analyzer', { cpu: 3.9, memory: 1.9 * GB, stubborn: true })
]

export const MACHINE: Required<NonNullable<FakeOptions['machine']>> = {
  cpu: {
    sampled: true,
    usage: 34,
    cores: [62, 18, 41, 27, 55, 9, 33, 29].map((usage) => ({ usage })),
    logicalCores: 8,
    physicalCores: 4,
    brand: 'Apple M2',
    loadAverage: { one: 1.2, five: 0.9, fifteen: 0.7 }
  },
  memory: { total: 16 * GB, used: 9.8 * GB, available: 6.2 * GB },
  swap: { total: 2 * GB, used: 0.4 * GB },
  disks: [
    { name: 'nvme0n1p2', mountPoint: '/', fileSystem: 'ext4', totalBytes: 512 * GB, availableBytes: 194 * GB, readRate: 14 * MB, writeRate: 3.2 * MB },
    { name: 'nvme1n1', mountPoint: '/data', fileSystem: 'ext4', totalBytes: 2048 * GB, availableBytes: 920 * GB, readRate: 0, writeRate: 0 }
  ],
  networks: [
    { name: 'eth0', receiveRate: 1.2 * MB, transmitRate: 84 * 1024 },
    { name: 'wlan0', receiveRate: 0, transmitRate: 0 }
  ],
  machine: { name: 'homelab', osName: 'Ubuntu', osVersion: '24.04', uptimeSecs: 3 * 86400 + 4 * 3600, arch: 'x86_64' }
}

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const machine = { ...MACHINE, ...options.machine }
  const pace = options.pace ?? 0
  const canTerminate = !options.cannotTerminate
  let programs: Program[] = (options.programs ?? PROGRAMS).map((p) => ({ ...p }))
  let pins: ProcessRef[] = []
  const signals: string[] = []
  let readings = 0
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

  const find = (ref: ProcessRef): Program | undefined => programs.find((p) => p.pid === ref.pid && p.startTime === ref.startTime)
  const held = (ref: ProcessRef): boolean => pins.some((p) => p.pid === ref.pid && p.startTime === ref.startTime)
  // a process that was chosen and has exited is still known to the backend, as it was
  const known = new Map<string, Program>()
  const keyOf = (ref: ProcessRef): string => `${ref.pid}:${ref.startTime}`

  // rates and loads need two readings: the first has none
  const sampled = (): boolean => readings >= 2
  const rowOf = (p: Program): ProcessRow => ({
    pid: p.pid,
    startTime: p.startTime,
    name: p.name,
    ...(p.user === undefined ? {} : { user: p.user }),
    ...(sampled() && p.cpu !== undefined ? { cpu: p.cpu } : {}),
    memory: p.memory,
    ...(p.status === undefined ? {} : { status: p.status })
  })
  const related = (p: Program): { pid: number; startTime: number; name: string; user?: string } => ({
    pid: p.pid,
    startTime: p.startTime,
    name: p.name,
    ...(p.user === undefined ? {} : { user: p.user })
  })
  const detailOf = (ref: ProcessRef): Detail | undefined => {
    const p = find(ref)
    if (!p) {
      const was = known.get(keyOf(ref))
      return was ? { pid: was.pid, startTime: was.startTime, name: was.name, alive: false, command: [], children: [] } : undefined
    }
    const parent = programs.find((q) => q.pid === p.parent)
    return {
      pid: p.pid,
      startTime: p.startTime,
      name: p.name,
      alive: true,
      ...(p.status === undefined ? {} : { status: p.status }),
      ...(p.user === undefined ? {} : { user: p.user }),
      ...(sampled() && p.cpu !== undefined ? { cpu: p.cpu } : {}),
      memory: p.memory,
      ...(p.virtualMemory === undefined ? {} : { virtualMemory: p.virtualMemory }),
      runTimeSecs: 3600,
      command: p.command ?? [],
      ...(p.exe === undefined ? {} : { exe: p.exe }),
      ...(parent ? { parent: related(parent) } : {}),
      children: programs.filter((q) => q.parent === p.pid).map(related)
    }
  }
  const overview = (): Overview => {
    const cpu: Cpu = sampled()
      ? machine.cpu
      : { sampled: false, cores: machine.cpu.cores.map(() => ({})), logicalCores: machine.cpu.logicalCores, ...(machine.cpu.brand === undefined ? {} : { brand: machine.cpu.brand }) }
    const rates = (): boolean => sampled()
    return {
      cpu,
      memory: machine.memory,
      swap: machine.swap,
      disks: machine.disks.map((d) => (rates() ? d : { name: d.name, mountPoint: d.mountPoint, fileSystem: d.fileSystem, totalBytes: d.totalBytes, availableBytes: d.availableBytes })),
      networks: machine.networks.map((n) => (rates() ? n : { name: n.name })),
      machine: machine.machine,
      unsupported: [],
      processCount: programs.length,
      canTerminate
    }
  }
  const remember = (p: Program): void => void known.set(keyOf(p), p)

  const rpc: ProcessesRpc = {
    snapshot: (params) =>
      answered((): Snapshot => {
        readings += 1
        const snapshot: Snapshot = { overview: overview(), processes: programs.map(rowOf) }
        const selected = params?.selected
        if (selected) {
          const detail = held(selected) || find(selected) ? detailOf(selected) : undefined
          if (detail) snapshot.detail = detail
        }
        return snapshot
      }),
    select: ({ process }) =>
      answered(() => {
        const p = find(process)
        if (!p) throw new Refused('That program has quit.')
        if (!held(process)) pins = [...pins, { pid: process.pid, startTime: process.startTime }]
        remember(p)
        return detailOf(process) as Detail
      }),
    release: ({ process }) =>
      answered(() => {
        pins = pins.filter((p) => !(p.pid === process.pid && p.startTime === process.startTime))
        return { released: true }
      }),
    terminate: ({ process, force }) =>
      answered(() => {
        if (!held(process)) throw new Refused('Choose the program again: Processes no longer holds it.')
        const p = find(process)
        const was = known.get(keyOf(process))
        const name = p?.name ?? was?.name ?? 'The program'
        if (!p) {
          const reused = programs.some((q) => q.pid === process.pid)
          throw new Refused(reused ? `${name} has quit, and another program has its number now. Nothing was stopped.` : `${name} has already quit. Nothing was stopped.`)
        }
        if (p.own) throw new Refused('Processes runs inside this program, so it cannot stop it.')
        if (!force && !canTerminate) throw new Refused('This system cannot ask a program to quit. Use Force Terminate.')
        if (p.protected) {
          throw new Refused(`This system did not let Processes stop ${name}. It belongs to a user with more rights than yours: ask an administrator.`)
        }
        signals.push(`${force ? 'kill' : 'terminate'} ${p.pid}`)
        if (force || !p.stubborn) programs = programs.filter((q) => q !== p)
        return { sent: true, force }
      })
  }

  const events: ProcessesEvents = {
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
    quit(pid) {
      const p = programs.find((q) => q.pid === pid)
      if (p) remember(p)
      programs = programs.filter((q) => q.pid !== pid)
    },
    reuse(pid, next) {
      const p = programs.find((q) => q.pid === pid)
      if (p) remember(p)
      programs = [...programs.filter((q) => q.pid !== pid), { ...next, pid }]
    },
    start(next) {
      programs = [...programs, { ...next }]
    },
    signals: () => signals,
    held: () => pins,
    readings: () => readings,
    restart() {
      pins = []
    }
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: { snapshot: refuse, select: refuse, release: refuse, terminate: refuse },
    events: { on: () => () => {} },
    emit: () => {},
    quit: () => {},
    reuse: () => {},
    start: () => {},
    signals: () => [],
    held: () => [],
    readings: () => 0,
    restart: () => {}
  }
}
