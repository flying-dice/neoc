import { describe, expect, test } from 'bun:test'
import { Dns } from './dns.svelte'
import { PORT_TAKEN, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Dns> {
  const dns = new Dns(backend.rpc, backend.events)
  dns.listen()
  await dns.load()
  return dns
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const names = (dns: Dns): string[] => dns.records.map((r) => `${r.name} ${r.address}`)

describe('a window that has just opened', () => {
  test('says nothing about Docker until the first answer', () => {
    const backend = createFakeBackend()
    const dns = new Dns(backend.rpc, backend.events)
    expect(dns.loaded).toBe(false)
    expect(dns.records).toEqual([])
  })

  test('shows the server, its settings and the records by name', async () => {
    const dns = await opened()
    expect(dns.loaded).toBe(true)
    expect(dns.engine).toEqual({ reachable: true, version: '27.3.1', host: '', problem: '' })
    expect(dns.server).toMatchObject({ enabled: true, state: 'running', current: true })
    expect(dns.settings).toEqual({ port: 53, upstreams: ['1.1.1.1', '1.0.0.1'], logQueries: false })
    expect(names(dns)).toEqual(['apps.home 192.168.1.20', 'nas.home 192.168.1.10', 'nas.home fd00::10', 'printer.home 192.168.1.31'])
    expect(dns.failure).toBeNull()
  })

  test('says why when Docker cannot be reached, and still has the records', async () => {
    const dns = await opened(createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }))
    expect(dns.engine.reachable).toBe(false)
    expect(dns.server.state).toBe('absent')
    expect(dns.records).toHaveLength(4)
    // Docker being away is an answer, not a failed call
    expect(dns.failure).toBeNull()
  })

  test('keeps the reason when the backend itself cannot be reached', async () => {
    const dns = await opened(createFailingBackend('The host is not answering.'))
    expect(dns.failure).toBe('The host is not answering.')
    expect(dns.loaded).toBe(false)
  })
})

describe('the server', () => {
  test('is turned off, which removes its container and keeps the records', async () => {
    const dns = await opened()
    expect(await dns.setEnabled(false)).toBe(true)
    expect(dns.server).toMatchObject({ enabled: false, state: 'absent', current: false })
    expect(dns.records).toHaveLength(4)
  })

  test('is turned on, which makes its container', async () => {
    const backend = createFakeBackend({ enabled: false })
    const dns = await opened(backend)
    expect(dns.server.state).toBe('absent')
    await dns.setEnabled(true)
    expect(dns.server).toMatchObject({ enabled: true, state: 'running', current: true })
    expect(backend.made()).toBe(1)
  })

  test('is working while its container is made, and not after', async () => {
    const dns = await opened(createFakeBackend({ enabled: false, pace: 5 }))
    const starting = dns.setEnabled(true)
    expect(dns.working).toBe(true)
    await starting
    expect(dns.working).toBe(false)
  })

  test('that Docker refuses to start is wanted on, not running, and says why: the call did not fail', async () => {
    const dns = await opened(createFakeBackend({ enabled: false, refusing: PORT_TAKEN }))
    expect(await dns.setEnabled(true)).toBe(true)
    expect(dns.failure).toBeNull()
    expect(dns.server).toMatchObject({ enabled: true, state: 'stopped' })
    expect(dns.server.problem).toContain('port is already allocated')
  })

  test('that was stopped in a terminal is seen, and started again when asked', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    backend.outside('stopped')
    await until(() => dns.server.state === 'stopped')
    await dns.apply()
    expect(dns.server.state).toBe('running')
  })
})

describe('a record', () => {
  test('is added as it is typed, the server made again with it, and the fields emptied', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    dns.editRecord({ name: ' TV.Home. ' })
    dns.editRecord({ address: '192.168.1.40' })
    expect(await dns.addRecord()).toBe(true)
    expect(names(dns)).toContain('tv.home 192.168.1.40')
    expect(dns.record).toEqual({ name: '', address: '', subdomains: false })
    expect(backend.made()).toBe(1)
    expect(dns.server.current).toBe(true)
  })

  test('with a name or an address that is none says so at its field, stays typed, and is not sent', async () => {
    const backend = createFakeBackend()
    let sent = 0
    const rpc = { ...backend.rpc, addRecord: (params: Parameters<typeof backend.rpc.addRecord>[0]) => (sent++, backend.rpc.addRecord(params)) }
    const dns = new Dns(rpc, backend.events)
    await dns.load()
    dns.editRecord({ name: 'tv_home', address: '192.168.1.40' })
    expect(dns.recordProblems).toEqual({ name: 'Only letters, digits, hyphens and dots, like nas.home.', address: null })
    expect(await dns.addRecord()).toBe(false)
    expect(dns.record.name).toBe('tv_home')
    dns.editRecord({ name: 'tv.home', address: '192.168.1.400' })
    expect(dns.recordProblems).toEqual({ name: null, address: 'Not an IP address, like 192.168.1.10 or fd00::10.' })
    expect(await dns.addRecord()).toBe(false)
    expect(sent).toBe(0)
    // what is typed wrong is the field's to say, not the window's
    expect(dns.failure).toBeNull()
    expect(dns.records).toHaveLength(4)
    // put right, it is added
    dns.editRecord({ address: '192.168.1.40' })
    expect(dns.recordProblems).toEqual({ name: null, address: null })
    expect(await dns.addRecord()).toBe(true)
    expect(sent).toBe(1)
  })

  test('with a field left empty is not sent, and nothing is wrong with it yet', async () => {
    const dns = await opened()
    dns.editRecord({ name: 'tv.home' })
    expect(dns.recordProblems).toEqual({ name: null, address: null })
    expect(await dns.addRecord()).toBe(false)
    expect(dns.records).toHaveLength(4)
  })

  test('that is there already is refused by the backend, which the form says until it is typed again', async () => {
    const dns = await opened()
    dns.editRecord({ name: 'nas.home', address: '192.168.1.10' })
    expect(await dns.addRecord()).toBe(false)
    expect(dns.recordRefused).toBe('nas.home has the address 192.168.1.10 already.')
    expect(dns.failure).toBeNull()
    expect(dns.record.name).toBe('nas.home')
    dns.editRecord({ address: '192.168.1.11' })
    expect(dns.recordRefused).toBeNull()
  })

  test('that could not be added because the host is away is the window\'s to say, not the form\'s', async () => {
    const backend = createFakeBackend()
    const dns = new Dns({ ...backend.rpc, addRecord: () => Promise.reject(new Error('The host is not answering.')) }, backend.events)
    await dns.load()
    dns.editRecord({ name: 'tv.home', address: '192.168.1.40' })
    expect(await dns.addRecord()).toBe(false)
    expect(dns.failure).toBe('The host is not answering.')
    expect(dns.recordRefused).toBeNull()
  })

  test('is kept while the server is off, with no container made', async () => {
    const backend = createFakeBackend({ enabled: false })
    const dns = await opened(backend)
    dns.editRecord({ name: 'tv.home', address: '192.168.1.40', subdomains: true })
    await dns.addRecord()
    expect(dns.records.find((r) => r.name === 'tv.home')).toMatchObject({ subdomains: true })
    expect(backend.made()).toBe(0)
    expect(dns.server.state).toBe('absent')
  })

  test('is removed, and busy only while the call is in flight', async () => {
    const dns = await opened(createFakeBackend({ pace: 5 }))
    const printer = dns.records.find((r) => r.name === 'printer.home')!
    const removing = dns.removeRecord(printer)
    expect(dns.removing(printer)).toBe(true)
    expect(dns.removing(dns.records[0]!)).toBe(false)
    expect(await removing).toBe(true)
    expect(dns.removing(printer)).toBe(false)
    expect(names(dns)).not.toContain('printer.home 192.168.1.31')
  })

  test('added elsewhere appears with no input', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    backend.elsewhere('cam.home', '192.168.1.50')
    await until(() => names(dns).includes('cam.home 192.168.1.50'))
  })
})

describe('the settings', () => {
  test('are shown as they are kept until something is typed', async () => {
    const dns = await opened()
    expect(dns.settingsDraft).toEqual({ port: '53', upstreams: '1.1.1.1, 1.0.0.1', logQueries: false })
    expect(dns.settingsChanged).toBe(false)
    dns.editSettings({ port: '5353' })
    expect(dns.settingsChanged).toBe(true)
    // typing it back is no change
    dns.editSettings({ port: '53' })
    expect(dns.settingsChanged).toBe(false)
  })

  test('are saved, and the server made again with them', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    dns.editSettings({ port: '5353', upstreams: '9.9.9.9 149.112.112.112', logQueries: true })
    expect(await dns.saveSettings()).toBe(true)
    expect(dns.settings).toEqual({ port: 5353, upstreams: ['9.9.9.9', '149.112.112.112'], logQueries: true })
    expect(dns.settingsChanged).toBe(false)
    expect(backend.made()).toBe(1)
  })

  test('have nothing wrong with them as they are kept', async () => {
    const dns = await opened()
    expect(dns.settingsProblems).toEqual({ port: null, upstreams: null })
  })

  test('with a port that is none say so at the port, and are not sent', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    dns.editSettings({ port: '70000' })
    expect(dns.settingsProblems.port).toBe('A number from 1 to 65535.')
    expect(await dns.saveSettings()).toBe(false)
    expect(dns.failure).toBeNull()
    expect(dns.settings.port).toBe(53)
    expect(backend.made()).toBe(0)
  })

  test('with an upstream server that is no address say so at the servers, are not sent, and stay typed', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    dns.editSettings({ upstreams: '9.9.9.9, one.one.one.one' })
    expect(dns.settingsProblems.upstreams).toBe('one.one.one.one is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.')
    expect(await dns.saveSettings()).toBe(false)
    expect(dns.failure).toBeNull()
    expect(backend.made()).toBe(0)
    expect(dns.settingsDraft.upstreams).toBe('9.9.9.9, one.one.one.one')
    dns.resetSettings()
    expect(dns.settingsDraft.upstreams).toBe('1.1.1.1, 1.0.0.1')
    expect(dns.settingsProblems.upstreams).toBeNull()
  })
})

describe('the log', () => {
  test('is read when it is shown, and again when asked', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    expect(dns.logs).toBeNull()
    await dns.show('log')
    expect(dns.logs?.text).toContain('dnsmasq: started')
    expect(dns.following).toBe(true)
    backend.write('dnsmasq: query[A] nas.home from 192.168.1.7')
    await dns.refreshLog()
    expect(dns.logs?.text).toContain('query[A] nas.home')
  })

  test('of a server that is off is nothing, and no failed call', async () => {
    const dns = await opened(createFakeBackend({ enabled: false }))
    await dns.show('log')
    expect(dns.logs).toBeNull()
    expect(dns.failure).toBeNull()
    expect(dns.following).toBe(false)
  })

  test('is not read while something else is shown', async () => {
    const backend = createFakeBackend()
    const dns = await opened(backend)
    await dns.refreshLog()
    expect(dns.logs).toBeNull()
  })
})

describe('another Docker', () => {
  test('is chosen by its address, and this machine by none', async () => {
    const dns = await opened()
    dns.hostDraft = ' tcp://nas.local:2375 '
    await dns.connect()
    expect(dns.engine).toMatchObject({ reachable: false, host: 'tcp://nas.local:2375' })
    expect(dns.hostDraft).toBe('tcp://nas.local:2375')
    dns.hostDraft = ''
    await dns.connect()
    expect(dns.engine).toMatchObject({ reachable: true, host: '' })
  })

  test('with an address that is none is refused, and the one chosen stays', async () => {
    const dns = await opened()
    dns.hostDraft = 'nas.local'
    await dns.connect()
    expect(dns.failure).toContain('must start with unix:///')
    expect(dns.engine.host).toBe('')
  })
})

test('a host connection that came back is read again', async () => {
  const backend = createFakeBackend()
  const dns = await opened(backend)
  await backend.rpc.removeRecord({ record: 3 })
  expect(dns.records).toHaveLength(4)
  backend.emit('host:reconnected')
  await until(() => dns.records.length === 3)
})
