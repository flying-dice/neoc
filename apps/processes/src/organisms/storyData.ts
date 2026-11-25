/** What the organisms' stories draw: a home lab as one reading of it, written out (the page's stories run a fake backend). */
import type { Detail, Overview, ProcessRow } from 'app:rpc'
import { MACHINE, PROGRAMS } from '../lib/fakeApi'

/** The machine after two readings: every rate is there. */
export const OVERVIEW: Overview = {
  cpu: MACHINE.cpu,
  memory: MACHINE.memory,
  swap: MACHINE.swap,
  disks: MACHINE.disks,
  networks: MACHINE.networks,
  machine: MACHINE.machine,
  unsupported: [],
  processCount: PROGRAMS.length,
  canTerminate: true
}

/** The machine after one: sizes are there, rates are not. */
export const FIRST_READING: Overview = {
  ...OVERVIEW,
  cpu: { sampled: false, cores: MACHINE.cpu.cores.map(() => ({})), logicalCores: 8, brand: 'Apple M2' },
  disks: MACHINE.disks.map(({ name, mountPoint, fileSystem, totalBytes, availableBytes }) => ({ name, mountPoint, fileSystem, totalBytes, availableBytes })),
  networks: MACHINE.networks.map(({ name }) => ({ name }))
}

export const ROWS: readonly ProcessRow[] = PROGRAMS.map(({ pid, startTime, name, user, cpu, memory, status }) => ({
  pid,
  startTime,
  name,
  ...(user === undefined ? {} : { user }),
  ...(cpu === undefined ? {} : { cpu }),
  memory,
  ...(status === undefined ? {} : { status })
}))

const postgres = PROGRAMS.find((p) => p.pid === 812)!

/** The database, in full. */
export const POSTGRES: Detail = {
  pid: postgres.pid,
  startTime: postgres.startTime,
  name: postgres.name,
  alive: true,
  status: 'run',
  user: 'postgres',
  cpu: 6.1,
  memory: postgres.memory,
  virtualMemory: postgres.virtualMemory!,
  runTimeSecs: 3 * 3600 + 20 * 60,
  command: postgres.command ?? [],
  exe: postgres.exe!,
  parent: { pid: 1, startTime: 1_700_000_001, name: 'systemd', user: 'root' },
  children: [
    { pid: 841, startTime: 1_700_000_841, name: 'postgres: checkpointer', user: 'postgres' },
    { pid: 842, startTime: 1_700_000_842, name: 'postgres: background writer', user: 'postgres' }
  ]
}
