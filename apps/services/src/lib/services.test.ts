import { describe, expect, test } from 'bun:test'
import { NO_MANAGER, LAUNCHD, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'
import { counts, matches, search, summary, unsaid } from './format'
import { Services } from './services.svelte'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Services> {
  const services = new Services(backend.rpc, backend.events)
  services.listen()
  await services.load()
  return services
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const labels = (services: Services): string[] => services.services.map((s) => s.label)
const named = (services: Services, label: string) => {
  const found = services.all.find((s) => s.label === label)
  if (!found) throw new Error(`no ${label}`)
  return found
}

describe('a window that has just opened', () => {
  test('says nothing about the manager until the first answer', () => {
    const backend = createFakeBackend()
    const services = new Services(backend.rpc, backend.events)
    expect(services.loaded).toBe(false)
    expect(services.all).toEqual([])
  })

  test('shows every service the operating system has, by label, whoever registered it', async () => {
    const services = await opened()
    expect(labels(services)).toEqual(['backup', 'cups', 'nginx', 'postgres', 'systemd-logind'])
    expect(services.manager.kind).toBe('systemd')
    expect(services.failure).toBeNull()
  })

  test('says why when there is no service manager: an answer, not a failed call', async () => {
    const services = await opened(createFakeBackend({ problem: NO_MANAGER }))
    expect(services.manager).toMatchObject({ available: false, problem: NO_MANAGER })
    expect(services.all).toEqual([])
    expect(services.failure).toBeNull()
  })

  test('keeps the reason when the backend itself cannot be reached', async () => {
    const services = await opened(createFailingBackend('The host is not answering.'))
    expect(services.failure).toBe('The host is not answering.')
    expect(services.loaded).toBe(false)
  })
})

describe('filters and search', () => {
  test('each filter shows its own services and the counts say how many', async () => {
    const services = await opened()
    expect(counts(services.all)).toEqual({ all: 5, running: 3, stopped: 1, failed: 1, autostart: 3 })
    services.setFilter('running')
    expect(labels(services)).toEqual(['nginx', 'postgres', 'systemd-logind'])
    services.setFilter('failed')
    expect(labels(services)).toEqual(['backup'])
    services.setFilter('autostart')
    expect(labels(services)).toEqual(['backup', 'nginx', 'postgres'])
  })

  test('the search narrows by label or description, whatever the capitals', async () => {
    const services = await opened()
    services.setQuery('PRINT')
    expect(labels(services)).toEqual(['cups'])
    services.setQuery('  nginx ')
    expect(labels(services)).toEqual(['nginx'])
    services.setQuery('nothing like it')
    expect(labels(services)).toEqual([])
  })

  test('words for the list and for what a manager does not say', async () => {
    const services = await opened()
    expect(summary(services.all)).toBe('3 running of 5')
    expect(summary([])).toBe('No services')
    expect(search(services.all, '').length).toBe(5)
    expect(matches(named(services, 'cups'), 'stopped')).toBe(true)
    expect(unsaid(LAUNCHD, 'description')).toBe('Not Supported')
    expect(unsaid(LAUNCHD, 'program')).toBe('Not Available')
  })
})

describe('starting, stopping and restarting', () => {
  test('a service is started, stopped and restarted, and the overview is the operating system’s', async () => {
    const backend = createFakeBackend()
    const services = await opened(backend)
    expect(await services.control(named(services, 'cups'), 'start')).toBe(true)
    expect(named(services, 'cups').state).toBe('running')
    expect(await services.control(named(services, 'cups'), 'restart')).toBe(true)
    expect(named(services, 'cups').state).toBe('running')
    expect(await services.control(named(services, 'cups'), 'stop')).toBe(true)
    expect(named(services, 'cups').state).toBe('stopped')
    expect(services.notice).toBeNull()
    expect(backend.calls.filter((c) => c.endsWith('cups'))).toEqual(['start cups', 'restart cups', 'stop cups'])
  })

  test('a service is busy while the operating system is asked', async () => {
    const services = await opened(createFakeBackend({ pace: 20 }))
    const cups = named(services, 'cups')
    const started = services.control(cups, 'start')
    expect(services.busy(cups)).toBe(true)
    expect(services.busy(named(services, 'nginx'))).toBe(false)
    await started
    expect(services.busy(cups)).toBe(false)
  })

  test('a refusal of the operating system is said at the service, and nothing changes', async () => {
    const services = await opened()
    const logind = named(services, 'systemd-logind')
    services.select(logind)
    expect(await services.control(logind, 'stop')).toBe(false)
    expect(services.notice).toMatchObject({ status: 'denied', label: 'systemd-logind' })
    expect(services.notice?.message).toContain('administrator rights')
    expect(named(services, 'systemd-logind').state).toBe('running')
    expect(services.failure).toBeNull()
    // looking at another service leaves the sentence behind
    services.select(named(services, 'cups'))
    expect(services.notice).toBeNull()
  })

  test('a start that fails says so and shows the failed state', async () => {
    const services = await opened()
    expect(await services.control(named(services, 'backup'), 'start')).toBe(false)
    expect(services.notice).toMatchObject({ status: 'failed', label: 'backup' })
    expect(named(services, 'backup').state).toBe('failed')
  })

  test('a service another program removed is “missing”, and leaves the list', async () => {
    const backend = createFakeBackend()
    const services = await opened(backend)
    const cups = named(services, 'cups')
    services.select(cups)
    backend.outside((all) => all.filter((s) => s.label !== 'cups'))
    expect(await services.control(cups, 'start')).toBe(false)
    expect(services.notice?.status).toBe('missing')
    expect(services.selected).toBeUndefined()
  })
})

describe('what happens behind the app’s back', () => {
  test('a change made by another program is seen at the next ask', async () => {
    const backend = createFakeBackend()
    const services = await opened(backend)
    backend.outside((all) => all.map((s) => (s.label === 'cups' ? { ...s, state: 'running' as const } : s)).concat({ label: 'newcomer', state: 'running' as const }))
    expect(named(services, 'cups').state).toBe('stopped')
    await services.load()
    expect(named(services, 'cups').state).toBe('running')
    expect(labels(services)).toContain('newcomer')
  })

  test('the host reconnecting asks again', async () => {
    const backend = createFakeBackend()
    const services = await opened(backend)
    backend.outside((all) => all.filter((s) => s.label !== 'cups'))
    backend.emit('host:reconnected')
    await until(() => !labels(services).includes('cups'))
  })
})

describe('changing settings', () => {
  test('only what is given changes', async () => {
    const services = await opened()
    const before = named(services, 'nginx')
    expect(await services.configure(before, { autostart: false })).toBe(true)
    expect(named(services, 'nginx')).toEqual({ ...before, autostart: false })
    expect(await services.configure(named(services, 'nginx'), { restart: 'always', description: 'Front door' })).toBe(true)
    expect(named(services, 'nginx')).toEqual({ ...before, autostart: false, restart: 'always', description: 'Front door' })
  })

  test('a setting the manager does not keep is unsupported, and nothing changes', async () => {
    const services = await opened(createFakeBackend({ manager: LAUNCHD }))
    expect(await services.configure(named(services, 'nginx'), { description: 'x' })).toBe(false)
    expect(services.notice?.status).toBe('unsupported')
    expect(named(services, 'nginx').description).toBeUndefined()
    expect(services.changes('autostart')).toBe(true)
    expect(services.changes('description')).toBe(false)
  })
})

describe('registering a service', () => {
  test('is possible once there is a name and a program, and the service is shown as the manager has it', async () => {
    const services = await opened()
    services.openForm()
    expect(services.ready).toBe(false)
    services.edit({ label: ' my-app ', program: ' /usr/local/bin/my-app ', args: '--serve\n\n  --port 80  ', autostart: true })
    expect(services.ready).toBe(true)
    expect(await services.register()).toBe(true)
    expect(services.registering).toBe(false)
    expect(services.selected?.label).toBe('my-app')
    expect(named(services, 'my-app')).toMatchObject({ program: '/usr/local/bin/my-app', args: ['--serve', '--port 80'], autostart: true, state: 'stopped' })
    expect(services.draft.label).toBe('')
  })

  test('the environment and the restart settings typed are registered and read back', async () => {
    const services = await opened()
    services.openForm()
    services.edit({ label: 'env-app', program: '/bin/env-app', environment: 'A=1\nB=two words=ok\n', restart: 'always', restartDelay: '7' })
    expect(await services.register()).toBe(true)
    expect(named(services, 'env-app')).toMatchObject({
      environment: [{ name: 'A', value: '1' }, { name: 'B', value: 'two words=ok' }],
      restart: 'always',
      restartDelaySecs: 7
    })
  })

  test('a line of the environment that is not NAME=value is refused at the form, before anything is sent', async () => {
    const services = await opened()
    services.openForm()
    services.edit({ label: 'env-app', program: '/bin/env-app', environment: 'A=1\nnot a variable' })
    expect(await services.register()).toBe(false)
    expect(services.notice).toMatchObject({ status: 'invalid', label: '' })
    expect(services.notice?.message).toContain('not a variable')
    services.edit({ environment: 'A=1', restartDelay: 'soon' })
    expect(await services.register()).toBe(false)
    expect(services.notice?.message).toContain('whole number')
  })

  test('what the operating system refused is said at the form and what was typed stays', async () => {
    const services = await opened()
    services.openForm()
    services.edit({ label: 'bad name', program: 'relative' })
    expect(await services.register()).toBe(false)
    expect(services.notice).toMatchObject({ status: 'invalid', label: '' })
    expect(services.registering).toBe(true)
    expect(services.draft.label).toBe('bad name')
    // typing again takes the sentence away
    services.edit({ label: 'good-name' })
    expect(services.notice).toBeNull()
  })

  test('a name that is taken is refused and the form stays', async () => {
    const services = await opened()
    services.openForm()
    services.edit({ label: 'nginx', program: '/usr/sbin/nginx' })
    expect(await services.register()).toBe(false)
    expect(services.notice?.message).toContain('there already')
  })

  test('a manager that cannot keep a description refuses one and says so', async () => {
    const services = await opened(createFakeBackend({ manager: LAUNCHD }))
    expect(services.keeps('description')).toBe(false)
    services.openForm()
    services.edit({ label: 'x', program: '/bin/x', description: 'nope' })
    expect(await services.register()).toBe(false)
    expect(services.notice?.status).toBe('unsupported')
  })

  test('Cancel closes the form and forgets what was typed', async () => {
    const services = await opened()
    services.openForm()
    services.edit({ label: 'half', program: '/bin/half' })
    services.closeForm()
    expect(services.registering).toBe(false)
    expect(services.draft.label).toBe('')
  })
})

describe('unregistering a service', () => {
  test('is asked about first; Keep leaves it, Unregister removes it', async () => {
    const services = await opened()
    const cups = named(services, 'cups')
    services.select(cups)
    services.ask(cups)
    expect(services.asking?.label).toBe('cups')
    services.keep()
    expect(services.asking).toBeUndefined()
    expect(labels(services)).toContain('cups')
    services.ask(cups)
    expect(await services.unregister()).toBe(true)
    expect(labels(services)).not.toContain('cups')
    expect(services.selected).toBeUndefined()
  })

  test('one the system will not let go of is said so, and stays', async () => {
    const services = await opened()
    const logind = named(services, 'systemd-logind')
    services.select(logind)
    services.ask(logind)
    expect(await services.unregister()).toBe(false)
    expect(services.notice?.status).toBe('denied')
    expect(labels(services)).toContain('systemd-logind')
    expect(services.selected?.label).toBe('systemd-logind')
  })
})
