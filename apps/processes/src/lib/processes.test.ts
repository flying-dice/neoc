import { describe, expect, test } from 'bun:test'
import { createFailingBackend, createFakeBackend, program, PROGRAMS, type FakeBackend } from './fakeApi'
import { LOST, matching, Processes, same, sorted } from './processes.svelte'

/** A window on a fake backend, listening, with two readings taken (the first has no rates). */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Processes> {
  const processes = new Processes(backend.rpc, backend.events)
  processes.listen()
  await processes.load()
  await processes.refresh()
  return processes
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const POSTGRES = { pid: 812, startTime: program(812, '').startTime }
const SSHD = { pid: 702, startTime: program(702, '').startTime }
const SERVER = { pid: 1204, startTime: program(1204, '').startTime }
const RUST = { pid: 3051, startTime: program(3051, '').startTime }
const names = (processes: Processes): string[] => processes.rows.map((row) => row.name)

describe('a window that has just opened', () => {
  test('shows nothing of the machine until the first reading', () => {
    const backend = createFakeBackend()
    const processes = new Processes(backend.rpc, backend.events)
    expect(processes.loaded).toBe(false)
    expect(processes.overview).toBeNull()
    expect(processes.rows).toEqual([])
    expect(processes.view).toBe('processes')
  })

  test('has no rates on the first reading: they need two', async () => {
    const processes = new Processes(createFakeBackend().rpc, createFakeBackend().events)
    await processes.load()
    expect(processes.loaded).toBe(true)
    expect(processes.overview?.cpu.sampled).toBe(false)
    expect(processes.overview?.cpu.usage).toBeUndefined()
    expect(processes.overview?.disks.every((disk) => disk.readRate === undefined)).toBe(true)
    expect(processes.processes.every((row) => row.cpu === undefined)).toBe(true)
    // memory is there at once
    expect(processes.overview?.memory.total).toBeGreaterThan(0)
    expect(processes.overview?.processCount).toBe(PROGRAMS.length)
  })

  test('has them from the second', async () => {
    const processes = await opened()
    expect(processes.overview?.cpu.sampled).toBe(true)
    expect(processes.overview?.cpu.usage).toBe(34)
    expect(processes.processes.find((row) => row.pid === 812)?.cpu).toBe(6.1)
    expect(processes.updatedAt).not.toBeNull()
    expect(processes.failure).toBeNull()
  })

  test('keeps the reason when the backend cannot be reached', async () => {
    const backend = createFailingBackend('The host is not answering.')
    const processes = new Processes(backend.rpc, backend.events)
    await processes.load()
    expect(processes.failure).toBe('The host is not answering.')
    expect(processes.loaded).toBe(false)
  })

  test('forgets a failure once a reading gets through', async () => {
    let fail = true
    const backend = createFakeBackend()
    const processes = new Processes(
      { ...backend.rpc, snapshot: (params) => (fail ? Promise.reject(new Error('The host is not answering.')) : backend.rpc.snapshot(params)) },
      backend.events
    )
    await processes.load()
    expect(processes.failure).not.toBeNull()
    fail = false
    await processes.refresh()
    expect(processes.failure).toBeNull()
    expect(processes.loaded).toBe(true)
  })

  test('does not read again while a reading is on its way', async () => {
    const backend = createFakeBackend({ pace: 20 })
    const processes = new Processes(backend.rpc, backend.events)
    await Promise.all([processes.refresh(), processes.refresh(), processes.refresh()])
    expect(backend.readings()).toBe(1)
  })

  test('reads again when the host connection comes back', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    const before = backend.readings()
    backend.start(program(4000, 'new-program'))
    backend.emit('host:reconnected')
    await until(() => processes.processes.some((row) => row.name === 'new-program'))
    expect(backend.readings()).toBe(before + 1)
  })

  test('stops listening when told to', async () => {
    const backend = createFakeBackend()
    const processes = new Processes(backend.rpc, backend.events)
    const stop = processes.listen()
    stop()
    backend.emit('host:reconnected')
    await Bun.sleep(10)
    expect(backend.readings()).toBe(0)
  })
})

describe('the list of processes', () => {
  test('is in the order of CPU, the busiest first, by default', async () => {
    const processes = await opened()
    expect(processes.sort).toBe('cpu')
    expect(names(processes).slice(0, 3)).toEqual(['luau-runtime', 'vos-server', 'postgres'])
  })

  test('puts a process that has no CPU yet after those that have, and breaks a tie by PID', () => {
    const rows = [
      { ...program(5, 'e'), cpu: undefined },
      { ...program(4, 'd'), cpu: 1 },
      { ...program(3, 'c'), cpu: 1 },
      { ...program(2, 'b'), cpu: 9 }
    ]
    expect(sorted(rows, 'cpu').map((row) => row.pid)).toEqual([2, 3, 4, 5])
  })

  test('can be put in the order of memory, PID or name', async () => {
    const processes = await opened()
    processes.order('memory')
    expect(names(processes).slice(0, 2)).toEqual(['rust-analyzer', 'vos-server'])
    processes.order('pid')
    expect(processes.rows.map((row) => row.pid).slice(0, 3)).toEqual([1, 391, 640])
    processes.order('name')
    expect(names(processes).slice(0, 3)).toEqual(['dockerd', 'luau-runtime', 'node'])
  })

  test('is narrowed by what is typed: part of a name, whatever its capitals, or the start of a PID', async () => {
    const processes = await opened()
    processes.search('POSTGRES')
    expect(names(processes)).toEqual(['postgres', 'postgres: checkpointer', 'postgres: background writer'])
    processes.search('  rust ')
    expect(names(processes)).toEqual(['rust-analyzer'])
    processes.search('70')
    expect(names(processes)).toEqual(['sshd'])
    processes.search('812')
    expect(names(processes)).toEqual(['postgres'])
    // a PID that only has the digits in it is not a match: 12 is not the start of 812, 1204
    processes.search('12')
    expect(names(processes)).toEqual(['vos-server'])
    processes.search('postgress')
    expect(processes.rows).toEqual([])
    processes.search('')
    expect(processes.rows.length).toBe(PROGRAMS.length)
  })

  test('is all of them for a search of blanks', () => {
    const rows = PROGRAMS.map((p) => ({ pid: p.pid, startTime: p.startTime, name: p.name, memory: p.memory }))
    expect(matching(rows, '   ')).toBe(rows)
  })

  test('says which processes are the same: a PID is not enough', () => {
    expect(same({ pid: 1, startTime: 5 }, { pid: 1, startTime: 5 })).toBe(true)
    expect(same({ pid: 1, startTime: 5 }, { pid: 1, startTime: 6 })).toBe(false)
    expect(same(null, { pid: 1, startTime: 5 })).toBe(false)
  })
})

describe('looking at a process', () => {
  test('chooses it: the backend holds it, and the window has its details and its relations', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    expect(await processes.choose(POSTGRES)).toBe(true)
    expect(backend.held()).toEqual([POSTGRES])
    expect(processes.selected).toEqual(POSTGRES)
    expect(processes.detail?.name).toBe('postgres')
    expect(processes.detail?.parent?.name).toBe('systemd')
    expect(processes.detail?.children.map((child) => child.name)).toEqual(['postgres: checkpointer', 'postgres: background writer'])
    expect(processes.detail?.command[0]).toBe('/usr/lib/postgresql/16/bin/postgres')
    expect(processes.endable).toBe(true)
    expect(processes.pane).toBe('detail')
  })

  test('lets the one chosen before go when another is chosen', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    await processes.choose(SSHD)
    await until(() => backend.held().length === 1)
    expect(backend.held()).toEqual([SSHD])
    expect(processes.detail?.name).toBe('sshd')
  })

  test('lets it go when it is closed, and shows nothing of it', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    processes.close()
    await until(() => backend.held().length === 0)
    expect(processes.selected).toBeNull()
    expect(processes.detail).toBeNull()
  })

  test('follows it through the readings', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    backend.start(program(900, 'worker', { parent: 812 }))
    await processes.refresh()
    expect(processes.detail?.children.map((child) => child.name)).toContain('worker')
  })

  test('says it has quit when it has, and it can no longer be ended', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    backend.quit(812)
    await processes.refresh()
    expect(processes.detail?.alive).toBe(false)
    expect(processes.detail?.name).toBe('postgres')
    expect(processes.endable).toBe(false)
    processes.ask('force')
    expect(processes.asking).toBeNull()
  })

  test('says so when the process quit after the list was read, and reads the list again', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    backend.quit(812)
    expect(await processes.choose(POSTGRES)).toBe(false)
    expect(processes.failure).toBe('That program has quit.')
    expect(processes.selected).toBeNull()
    expect(processes.pane).toBe('content')
    await until(() => !processes.processes.some((row) => row.pid === 812))
  })

  test('keeps looking at the one chosen when another cannot be', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    backend.quit(702)
    expect(await processes.choose(SSHD)).toBe(false)
    expect(processes.selected).toEqual(POSTGRES)
    expect(processes.detail?.name).toBe('postgres')
  })

  test('does not ask the backend again for a process it is looking at', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    processes.show('overview')
    const before = backend.readings()
    expect(await processes.choose(POSTGRES)).toBe(true)
    expect(backend.readings()).toBe(before)
    expect(processes.pane).toBe('detail')
  })

  test('lets go of a process that was chosen too late: another was chosen while it was asked for', async () => {
    const backend = createFakeBackend({ pace: 10 })
    const processes = await opened(backend)
    const first = processes.choose(POSTGRES)
    const second = processes.choose(SSHD)
    expect(await Promise.all([first, second])).toEqual([false, true])
    await until(() => backend.held().length === 1)
    expect(backend.held()).toEqual([SSHD])
    expect(processes.detail?.name).toBe('sshd')
  })
})

describe('ending a process', () => {
  test('asks first, and sends nothing when the answer is Cancel', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    processes.ask('force')
    expect(processes.asking).toBe('force')
    processes.keep()
    expect(processes.asking).toBeNull()
    expect(await processes.confirm()).toBe(false)
    expect(backend.signals()).toEqual([])
  })

  test('asking a process to quit sends the request, not the force, and says so', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    processes.ask('terminate')
    expect(await processes.confirm()).toBe(true)
    expect(backend.signals()).toEqual(['terminate 812'])
    expect(processes.notice).toBe('Asked postgres to quit.')
    expect(processes.asking).toBeNull()
    expect(processes.refused).toBeNull()
    await until(() => processes.detail?.alive === false)
    expect(processes.processes.some((row) => row.pid === 812)).toBe(false)
  })

  test('a process that ignores the request is still there, and the window does not turn the request into a force', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(RUST)
    processes.ask('terminate')
    await processes.confirm()
    await until(() => backend.readings() >= 3)
    expect(backend.signals()).toEqual(['terminate 3051'])
    expect(processes.detail?.alive).toBe(true)
    expect(processes.endable).toBe(true)
  })

  test('forcing stops it at once', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(RUST)
    processes.ask('force')
    expect(await processes.confirm()).toBe(true)
    expect(backend.signals()).toEqual(['kill 3051'])
    expect(processes.notice).toBe('Stopped rust-analyzer.')
    await until(() => processes.detail?.alive === false)
  })

  test('says what the system refused at the process, not as a failure of the call', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(SSHD)
    processes.ask('force')
    expect(await processes.confirm()).toBe(false)
    expect(processes.refused?.name).toBe('sshd')
    expect(processes.refused?.message).toContain('ask an administrator')
    expect(processes.failure).toBeNull()
    expect(processes.notice).toBeNull()
    expect(backend.signals()).toEqual([])
    // it is still there, and can be tried again
    expect(processes.endable).toBe(true)
  })

  test('forgets the refusal when it is asked about again, and when another process is chosen', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(SSHD)
    processes.ask('terminate')
    await processes.confirm()
    expect(processes.refused).not.toBeNull()
    processes.ask('terminate')
    expect(processes.refused).toBeNull()
    processes.keep()
    processes.ask('terminate')
    await processes.confirm()
    await processes.choose(POSTGRES)
    expect(processes.refused).toBeNull()
  })

  test('does not end the program the app runs in', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(SERVER)
    processes.ask('force')
    await processes.confirm()
    expect(processes.refused?.message).toBe('Processes runs inside this program, so it cannot stop it.')
    expect(backend.signals()).toEqual([])
  })

  test('does not end a process that has taken the PID of the one that was chosen', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    // the chosen process exits and another starts with its PID, before the answer to the question
    processes.ask('force')
    backend.reuse(812, program(812, 'innocent', { startTime: 1_800_000_000 }))
    expect(await processes.confirm()).toBe(false)
    expect(backend.signals()).toEqual([])
    expect(processes.refused?.message).toBe('postgres has quit, and another program has its number now. Nothing was stopped.')
    // the reading that follows the answer
    await until(() => processes.processes.find((row) => row.pid === 812)?.name === 'innocent')
    await until(() => processes.detail?.alive === false)
    expect(processes.detail?.name).toBe('postgres')
  })

  test('does not choose the process again for a backend that started afresh: the choice is refused and nothing is signalled', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    backend.restart()
    expect(backend.held()).toEqual([])
    processes.ask('terminate')
    expect(await processes.confirm()).toBe(false)
    expect(processes.refused?.message).toBe('Choose the program again: Processes no longer holds it.')
    expect(backend.signals()).toEqual([])
    // nothing was chosen on the way
    expect(backend.held()).toEqual([])
  })

  test('says so when the backend no longer holds the choice, keeps what it showed, and offers no ending until it is chosen again', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    expect(processes.endable).toBe(true)
    backend.restart()
    await processes.refresh()
    expect(processes.detail?.name).toBe('postgres')
    expect(processes.detail?.alive).toBe(true)
    expect(processes.endable).toBe(false)
    expect(processes.unavailable).toBe(LOST)
    processes.ask('force')
    expect(processes.asking).toBeNull()
    // chosen again, it can be ended
    expect(await processes.choose(POSTGRES)).toBe(true)
    expect(processes.endable).toBe(true)
    processes.ask('force')
    expect(await processes.confirm()).toBe(true)
    expect(backend.signals()).toEqual(['kill 812'])
  })

  test('a choice the backend let go of cannot end a process, even one that is still running', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    const [id] = backend.ids()
    expect(id).toBeDefined()
    backend.evict(id as string)
    processes.ask('force')
    expect(await processes.confirm()).toBe(false)
    expect(processes.refused?.message).toBe('Choose the program again: Processes no longer holds it.')
    expect(backend.signals()).toEqual([])
  })

  test('two windows that look at the same process each hold their own choice: one closing does not take the other\'s', async () => {
    const backend = createFakeBackend()
    const first = await opened(backend)
    const second = await opened(backend)
    await first.choose(POSTGRES)
    await second.choose(POSTGRES)
    expect(backend.ids()).toHaveLength(2)
    first.close()
    await until(() => backend.ids().length === 1)
    await second.refresh()
    expect(second.endable).toBe(true)
    second.ask('force')
    expect(await second.confirm()).toBe(true)
    expect(backend.signals()).toEqual(['kill 812'])
  })

  test('looks at a process that cannot be held for ending, says why, and offers no ending', async () => {
    const backend = createFakeBackend({
      programs: [program(900, 'wininit', { user: 'SYSTEM', blocked: 'permission' }), program(901, 'idle', { startTime: 0, blocked: 'unknown' })]
    })
    const processes = await opened(backend)
    expect(await processes.choose({ pid: 900, startTime: program(900, '').startTime })).toBe(true)
    expect(processes.detail?.name).toBe('wininit')
    expect(processes.detail?.user).toBe('SYSTEM')
    expect(processes.endable).toBe(false)
    expect(processes.unavailable).toMatch(/did not let Processes stop wininit/)
    processes.ask('force')
    expect(processes.asking).toBeNull()

    expect(await processes.choose({ pid: 901, startTime: 0 })).toBe(true)
    expect(processes.detail?.name).toBe('idle')
    expect(processes.endable).toBe(false)
    expect(processes.unavailable).toMatch(/cannot tell idle apart/)
    expect(backend.signals()).toEqual([])
  })

  test('says when this system cannot ask a program to quit, as Windows', async () => {
    const backend = createFakeBackend({ cannotTerminate: true })
    const processes = await opened(backend)
    expect(processes.overview?.canTerminate).toBe(false)
    await processes.choose(POSTGRES)
    processes.ask('terminate')
    await processes.confirm()
    expect(processes.refused?.message).toBe('This system cannot ask a program to quit. Use Force Terminate.')
    processes.ask('force')
    expect(await processes.confirm()).toBe(true)
    expect(backend.signals()).toEqual(['kill 812'])
  })

  test('keeps the reason a call did not get through, and not as a refusal', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    await processes.choose(POSTGRES)
    const rpc = { ...backend.rpc, terminate: () => Promise.reject(new Error('The host is not answering.')) }
    const window = new Processes(rpc, backend.events)
    await window.refresh()
    await window.choose(POSTGRES)
    window.ask('force')
    expect(await window.confirm()).toBe(false)
    expect(window.failure).toBe('The host is not answering.')
    expect(window.refused).toBeNull()
  })
})

describe('moving about a narrow window', () => {
  test('starts at the list, and goes in to a process and back out', async () => {
    const backend = createFakeBackend()
    const processes = await opened(backend)
    expect(processes.pane).toBe('content')
    await processes.choose(POSTGRES)
    expect(processes.pane).toBe('detail')
    processes.back()
    expect(processes.pane).toBe('content')
    expect(processes.selected).toBeNull()
    await until(() => backend.held().length === 0)
    processes.back()
    expect(processes.pane).toBe('sidebar')
  })

  test('shows the machine as a whole in the column of its own, and goes back to the views', async () => {
    const processes = await opened()
    processes.show('overview')
    expect(processes.view).toBe('overview')
    expect(processes.pane).toBe('detail')
    processes.back()
    expect(processes.pane).toBe('sidebar')
    processes.show('processes')
    expect(processes.pane).toBe('content')
  })

  test('is read as often as the person chooses', async () => {
    const processes = await opened()
    expect(processes.interval).toBe(2)
    processes.every(0)
    expect(processes.interval).toBe(0)
    processes.every(5)
    expect(processes.interval).toBe(5)
  })
})
