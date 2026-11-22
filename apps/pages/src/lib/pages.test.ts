import { describe, expect, test } from 'bun:test'
import { NO_FOLDER, PORT_TAKEN, SITES, createFailingBackend, createFakeBackend, type FakeBackend } from './fakeApi'
import { Pages } from './pages.svelte'

/** A window on a fake backend, loaded and listening. */
async function opened(backend: FakeBackend = createFakeBackend()): Promise<Pages> {
  const pages = new Pages(backend.rpc, backend.host)
  pages.listen()
  await pages.load()
  return pages
}

/** Wait until `done()` holds (an event is answered by a call, on a later turn of the event loop). */
async function until(done: () => boolean): Promise<void> {
  for (let i = 0; i < 200 && !done(); i++) await Bun.sleep(2)
  expect(done()).toBe(true)
}

const names = (pages: Pages): string[] => pages.sites.map((s) => s.name)
const named = (pages: Pages, name: string) => pages.sites.find((s) => s.name === name)!

describe('a window that has just opened', () => {
  test('says nothing about Docker until the first answer', () => {
    const backend = createFakeBackend()
    const pages = new Pages(backend.rpc, backend.host)
    expect(pages.loaded).toBe(false)
    expect(pages.sites).toEqual([])
  })

  test('shows the server, its settings and the sites by name', async () => {
    const pages = await opened()
    expect(pages.loaded).toBe(true)
    expect(pages.engine).toEqual({ reachable: true, version: '27.3.1', host: '', problem: '' })
    expect(pages.server).toMatchObject({ wanted: true, state: 'running', current: true })
    expect(pages.settings).toEqual({ port: 80 })
    expect(names(pages)).toEqual(['Blog', 'Family Wiki', 'Old Portfolio'])
    expect(pages.failure).toBeNull()
  })

  test('says why when Docker cannot be reached, and still has the sites', async () => {
    const pages = await opened(createFakeBackend({ problem: 'Docker is not running on this machine, or cannot be reached.' }))
    expect(pages.engine.reachable).toBe(false)
    expect(pages.server.state).toBe('absent')
    expect(pages.sites).toHaveLength(3)
    // Docker being away is an answer, not a failed call
    expect(pages.failure).toBeNull()
  })

  test('keeps the reason when the backend itself cannot be reached', async () => {
    const pages = await opened(createFailingBackend('The host is not answering.'))
    expect(pages.failure).toBe('The host is not answering.')
    expect(pages.loaded).toBe(false)
  })
})

describe('a new site', () => {
  test('is added as it is typed, on from the start, the server made again with it, and the list shown again', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    pages.startAdding()
    expect(pages.editing).toEqual({ site: null, draft: { name: '', folder: '', port: '', hostnames: '' } })
    pages.editSite({ name: ' Photos ', folder: '/Users/me/Sites/photos/' })
    pages.editSite({ port: '8082', hostnames: 'Photos.Home, www.photos.home' })
    expect(await pages.saveSite()).toBe(true)
    expect(named(pages, 'Photos')).toMatchObject({
      folder: '/Users/me/Sites/photos',
      port: 8082,
      hostnames: ['photos.home', 'www.photos.home'],
      enabled: true
    })
    expect(pages.editing).toBeNull()
    expect(backend.made()).toBe(1)
    expect(pages.server.current).toBe(true)
  })

  test('with only hostnames has no port of its own', async () => {
    const pages = await opened()
    pages.startAdding()
    pages.editSite({ name: 'Notes', folder: '/srv/notes', hostnames: 'notes.home' })
    await pages.saveSite()
    expect(named(pages, 'Notes').port).toBe(0)
  })

  test('with a port that is none is refused before the backend is asked, and stays typed', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    pages.startAdding()
    pages.editSite({ name: 'Notes', folder: '/srv/notes', port: '99999' })
    expect(await pages.saveSite()).toBe(false)
    expect(pages.failure).toContain('1 to 65535')
    expect(pages.editing?.draft.port).toBe('99999')
    expect(pages.sites).toHaveLength(3)
  })

  test.each([
    [{ name: 'Notes', folder: 'notes', port: '8082' }, 'The folder must be a full path'],
    [{ name: 'Notes', folder: '/srv/notes', port: '8081' }, 'Port 8081 is the port of Blog.'],
    [{ name: 'Notes', folder: '/srv/notes', port: '80' }, 'Port 80 is the one the sites with hostnames share'],
    [{ name: 'Notes', folder: '/srv/notes', hostnames: 'wiki.home' }, 'wiki.home is a hostname of Family Wiki.'],
    [{ name: 'Notes', folder: '/srv/notes', hostnames: 'notes_home' }, 'notes_home is not a hostname'],
    [{ name: 'Notes', folder: '/', port: '8082' }, 'The whole disk cannot be a site']
  ])('that the backend refuses says why in its words, and stays typed: %o', async (typed, why) => {
    const pages = await opened()
    pages.startAdding()
    pages.editSite(typed)
    expect(await pages.saveSite()).toBe(false)
    expect(pages.failure).toContain(why)
    expect(pages.editing).not.toBeNull()
    expect(pages.sites).toHaveLength(3)
  })

  test('that is given up leaves the list as it was', async () => {
    const pages = await opened()
    pages.startAdding()
    pages.editSite({ name: 'Notes' })
    pages.cancelEditing()
    expect(pages.editing).toBeNull()
    expect(pages.sites).toHaveLength(3)
  })
})

describe('a site of the list', () => {
  test('is changed from what it is, and may keep its own port and names', async () => {
    const pages = await opened()
    pages.startEditing(named(pages, 'Blog'))
    expect(pages.editing).toEqual({ site: 1, draft: { name: 'Blog', folder: '/Users/me/Sites/blog', port: '8081', hostnames: 'blog.home' } })
    pages.editSite({ name: 'Journal', hostnames: 'blog.home, journal.home' })
    expect(await pages.saveSite()).toBe(true)
    expect(named(pages, 'Journal')).toMatchObject({ id: 1, port: 8081, hostnames: ['blog.home', 'journal.home'], enabled: true })
    expect(names(pages)).not.toContain('Blog')
  })

  test('is turned off: it is kept and not served, and the server is made again without it', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    expect(await pages.setSiteEnabled(named(pages, 'Blog'), false)).toBe(true)
    expect(named(pages, 'Blog').enabled).toBe(false)
    expect(pages.server).toMatchObject({ state: 'running', current: true })
    expect(backend.made()).toBe(1)
  })

  test('that is the last one on is turned off, and the container is gone', async () => {
    const pages = await opened(createFakeBackend({ sites: [SITES[0]!] }))
    await pages.setSiteEnabled(pages.sites[0]!, false)
    expect(pages.server).toMatchObject({ wanted: false, state: 'absent' })
    expect(pages.sites).toHaveLength(1)
  })

  test('that is off is changed without the server being made again', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    pages.startEditing(named(pages, 'Old Portfolio'))
    pages.editSite({ name: 'Portfolio' })
    await pages.saveSite()
    expect(backend.made()).toBe(0)
    expect(pages.server.current).toBe(true)
  })

  test('is busy while its call is in flight, and not after', async () => {
    const pages = await opened(createFakeBackend({ pace: 5 }))
    const blog = named(pages, 'Blog')
    const turning = pages.setSiteEnabled(blog, false)
    expect(pages.busy(blog)).toBe(true)
    expect(pages.busy(named(pages, 'Family Wiki'))).toBe(false)
    expect(pages.working).toBe(true)
    await turning
    expect(pages.busy(blog)).toBe(false)
    expect(pages.working).toBe(false)
  })

  test('is removed', async () => {
    const pages = await opened()
    expect(await pages.removeSite(named(pages, 'Family Wiki'))).toBe(true)
    expect(names(pages)).toEqual(['Blog', 'Old Portfolio'])
  })

  test('that is gone already says so', async () => {
    const pages = await opened()
    const wiki = named(pages, 'Family Wiki')
    await pages.removeSite(wiki)
    expect(await pages.removeSite(wiki)).toBe(false)
    expect(pages.failure).toBe('There is no such site.')
  })

  test('whose folder is not there says so, and the others are served', async () => {
    const pages = await opened(createFakeBackend({ missing: ['/Users/me/Sites/blog'] }))
    expect(named(pages, 'Blog').problem).toBe(NO_FOLDER)
    expect(named(pages, 'Family Wiki').problem).toBe('')
    expect(pages.server.state).toBe('running')
  })

  test('is opened outside the app', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    await pages.open('http://blog.home/')
    expect(backend.opened()).toEqual(['http://blog.home/'])
    expect(pages.failure).toBeNull()
  })
})

describe('the server', () => {
  test('that Docker refuses to start is not running and says why: the call did not fail', async () => {
    const pages = await opened(createFakeBackend({ sites: [], refusing: PORT_TAKEN }))
    pages.startAdding()
    pages.editSite({ name: 'Blog', folder: '/srv/blog', port: '8081' })
    expect(await pages.saveSite()).toBe(true)
    expect(pages.failure).toBeNull()
    expect(pages.sites).toHaveLength(1)
    expect(pages.server).toMatchObject({ wanted: true, state: 'stopped' })
    expect(pages.server.problem).toContain('port is already allocated')
  })

  test('that was stopped in a terminal is seen, and started again when asked', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    backend.outside('stopped')
    await until(() => pages.server.state === 'stopped')
    await pages.apply()
    expect(pages.server.state).toBe('running')
    expect(backend.made()).toBe(1)
  })

  test('shows a site added elsewhere', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    backend.elsewhere('Docs', '/srv/docs', 8085)
    await until(() => pages.sites.length === 4)
    expect(names(pages)).toContain('Docs')
  })

  test('is read again when the host connection comes back', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    backend.reach(false)
    await until(() => !pages.engine.reachable)
    backend.reach(true)
    backend.emit('host:reconnected')
    await until(() => pages.engine.reachable)
  })

  test('is not heard any more once the window stops listening', async () => {
    const backend = createFakeBackend()
    const pages = new Pages(backend.rpc, backend.host)
    const stop = pages.listen()
    await pages.load()
    stop()
    backend.elsewhere('Docs', '/srv/docs', 8085)
    await Bun.sleep(10)
    expect(pages.sites).toHaveLength(3)
  })
})

describe('the settings', () => {
  test('are as they are kept until something is typed', async () => {
    const pages = await opened()
    expect(pages.portDraft).toBe('80')
    expect(pages.settingsChanged).toBe(false)
    pages.editPort('8080')
    expect(pages.settingsChanged).toBe(true)
    pages.resetSettings()
    expect(pages.portDraft).toBe('80')
  })

  test('are saved, and the server made again on the new port', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    pages.editPort('8080')
    expect(await pages.saveSettings()).toBe(true)
    expect(pages.settings).toEqual({ port: 8080 })
    expect(pages.settingsChanged).toBe(false)
    expect(backend.made()).toBe(1)
  })

  test('refuse a port that is none, and the own port of a site', async () => {
    const pages = await opened()
    pages.editPort('http')
    expect(await pages.saveSettings()).toBe(false)
    expect(pages.failure).toBe('The port is a number from 1 to 65535.')
    pages.editPort('8081')
    expect(await pages.saveSettings()).toBe(false)
    expect(pages.failure).toBe('Port 8081 is the own port of Blog.')
    expect(pages.settings).toEqual({ port: 80 })
  })

  test('choose another Docker, and say when it cannot be reached', async () => {
    const pages = await opened()
    pages.hostDraft = ' tcp://nas.local:2375 '
    await pages.connect()
    expect(pages.engine).toMatchObject({ reachable: false, host: 'tcp://nas.local:2375', problem: 'Docker cannot be reached at tcp://nas.local:2375.' })
    expect(pages.hostDraft).toBe('tcp://nas.local:2375')
    pages.hostDraft = 'nas.local'
    await pages.connect()
    expect(pages.failure).toContain('The address must start with')
  })
})

describe('the log', () => {
  test('is read when it is shown, and again when the server changes', async () => {
    const backend = createFakeBackend()
    const pages = await opened(backend)
    expect(pages.logs).toBeNull()
    await pages.show('log')
    expect(pages.logs?.text).toContain('start worker processes')
    expect(pages.following).toBe(true)
    backend.write('GET / HTTP/1.1')
    backend.emit('changed')
    await until(() => pages.logs?.text.includes('GET / HTTP/1.1') === true)
  })

  test('is nothing while the server has no container, which is not a failure', async () => {
    const pages = await opened(createFakeBackend({ sites: [] }))
    await pages.show('log')
    expect(pages.logs).toBeNull()
    expect(pages.failure).toBeNull()
    expect(pages.following).toBe(false)
  })
})
