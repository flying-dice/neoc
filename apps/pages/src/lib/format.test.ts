import { describe, expect, test } from 'bun:test'
import type { Server } from '@pages/rpc'
import { IMAGE, NO_FOLDER, SITES, site } from './fakeApi'
import { addressesOf, attentionOf, hostnamesOf, isServing, machineOf, portOf, servedCount, serverLabel, serverTone, sitesSummary } from './format'

const server = (change: Partial<Server> = {}): Server => ({
  wanted: true,
  state: 'running',
  status: 'Up 2 hours',
  current: true,
  image: IMAGE,
  problem: '',
  ...change
})
const none = server({ wanted: false, state: 'absent', status: '', current: false })
const ALL_OFF = SITES.map((s) => ({ ...s, enabled: false }))

describe('how the server reads', () => {
  test('serving, when its container runs and was made from what is kept', () => {
    expect(isServing(server())).toBe(true)
    expect(servedCount(SITES)).toBe(2)
    expect(serverTone(server(), SITES)).toBe('on')
    expect(serverLabel(server(), SITES)).toBe('Serving 2 sites')
    expect(serverLabel(server(), [SITES[0]!])).toBe('Serving 1 site')
    expect(attentionOf(server(), SITES)).toBeNull()
  })

  test('off, with no site or with every site off: there is nothing to be wrong', () => {
    expect(serverTone(none, [])).toBe('off')
    expect(serverLabel(none, [])).toBe('No sites yet')
    expect(serverLabel(none, ALL_OFF)).toBe('Every site is off')
    expect(attentionOf(none, ALL_OFF)).toBeNull()
  })

  test('off, when the only site that is on has no folder: the site says so, not the server', () => {
    const lost = [{ ...SITES[0]!, problem: NO_FOLDER }]
    const wanted = { ...none, wanted: true }
    expect(serverTone(wanted, lost)).toBe('off')
    expect(serverLabel(wanted, lost)).toBe('Nothing to serve')
    expect(attentionOf(wanted, lost)).toBeNull()
  })

  test('turning off, while the container of the last site is still there', () => {
    expect(serverTone(server({ wanted: false }), ALL_OFF)).toBe('busy')
    expect(serverLabel(server({ wanted: false }), ALL_OFF)).toBe('Turning off')
  })

  test('not running, with what Docker said when it said something', () => {
    const refused = server({ state: 'stopped', status: 'Created', problem: 'port is already allocated' })
    expect(serverTone(refused, SITES)).toBe('bad')
    expect(serverLabel(refused, SITES)).toBe('Not running')
    expect(attentionOf(refused, SITES)).toEqual({ heading: 'The server is not running', message: 'port is already allocated' })
    expect(attentionOf(server({ state: 'stopped', status: 'Exited (1) 5 seconds ago' }), SITES)?.message).toBe(
      'Its container stopped: Exited (1) 5 seconds ago.'
    )
    expect(attentionOf(server({ state: 'absent', status: '', current: false }), SITES)?.message).toBe('Its container is gone.')
  })

  test('running with older settings, and restarting', () => {
    const old = server({ current: false })
    expect(serverTone(old, SITES)).toBe('busy')
    expect(serverLabel(old, SITES)).toBe('Running with older settings')
    expect(attentionOf(old, SITES)?.heading).toBe('The server is running with older settings')
    const again = server({ state: 'restarting' })
    expect(serverLabel(again, SITES)).toBe('Restarting')
    expect(attentionOf(again, SITES)?.heading).toBe('The server keeps restarting')
  })
})

describe('what is typed', () => {
  test('a port is a whole number from 1 to 65535', () => {
    expect(portOf(' 8081 ')).toBe(8081)
    expect(portOf('65535')).toBe(65535)
    for (const typed of ['', '0', '65536', '80a', '-1', '8.5', '123456']) expect(portOf(typed)).toBeUndefined()
  })

  test('hostnames are apart by commas or spaces', () => {
    expect(hostnamesOf(' blog.home, www.blog.home  wiki.home,')).toEqual(['blog.home', 'www.blog.home', 'wiki.home'])
    expect(hostnamesOf('  ')).toEqual([])
  })

  test('how many sites there are', () => {
    expect([0, 1, 3].map(sitesSummary)).toEqual(['No sites', '1 site', '3 sites'])
  })
})

describe('where a site is reached', () => {
  test('on the machine this window was opened at, for its own Docker or one behind a socket', () => {
    expect(machineOf('', 'nas.local')).toBe('nas.local')
    expect(machineOf('unix:///var/run/docker.sock', 'nas.local')).toBe('nas.local')
  })

  test('on the machine of a Docker elsewhere', () => {
    expect(machineOf('tcp://server.lan:2375', 'nas.local')).toBe('server.lan')
    expect(machineOf('http://192.168.1.5:2375', 'nas.local')).toBe('192.168.1.5')
    expect(machineOf('tcp://[fd00::5]:2375', 'nas.local')).toBe('[fd00::5]')
  })

  test('at its own port, and at each of its names on the shared port', () => {
    expect(addressesOf(SITES[0]!, { port: 80 }, 'nas.local')).toEqual([
      { label: 'nas.local:8081', url: 'http://nas.local:8081/' },
      { label: 'blog.home', url: 'http://blog.home/' }
    ])
  })

  test('with the shared port in the address when it is not the web\'s own', () => {
    expect(addressesOf(site(2, 'Wiki', '/srv/wiki', 0, ['wiki.home']), { port: 8080 }, 'nas.local')).toEqual([
      { label: 'wiki.home:8080', url: 'http://wiki.home:8080/' }
    ])
  })
})
