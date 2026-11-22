import { describe, expect, test } from 'bun:test'
import type { Server, Settings } from 'app:rpc'
import { attentionOf, isServing, portOf, recordsSummary, serverLabel, serverTone, upstreamsOf } from './format'

const SETTINGS: Settings = { port: 53, upstreams: ['1.1.1.1'], logQueries: false }
const server = (change: Partial<Server> = {}): Server => ({
  enabled: true,
  state: 'running',
  status: 'Up 2 hours',
  current: true,
  image: '4km3/dnsmasq:2.90-r3-alpine-3.22',
  problem: '',
  ...change
})

describe('how the server reads', () => {
  test('answering, when its container runs and was made from what is kept', () => {
    expect(isServing(server())).toBe(true)
    expect(serverTone(server())).toBe('on')
    expect(serverLabel(server(), { ...SETTINGS, port: 5353 })).toBe('Answering on port 5353')
    expect(attentionOf(server())).toBeNull()
  })

  test('off, when it is not wanted and has no container', () => {
    const off = server({ enabled: false, state: 'absent', status: '', current: false })
    expect(serverTone(off)).toBe('off')
    expect(serverLabel(off, SETTINGS)).toBe('Off')
    expect(attentionOf(off)).toBeNull()
  })

  test('in between, when it runs with older settings', () => {
    const stale = server({ current: false })
    expect(isServing(stale)).toBe(false)
    expect(serverTone(stale)).toBe('busy')
    expect(serverLabel(stale, SETTINGS)).toBe('Running with older settings')
    expect(attentionOf(stale)?.heading).toBe('The server is running with older settings')
  })

  test('not running, with what Docker said when it refused', () => {
    const refused = server({ state: 'stopped', status: 'Created', problem: 'port is already allocated' })
    expect(serverTone(refused)).toBe('bad')
    expect(serverLabel(refused, SETTINGS)).toBe('Not running')
    expect(attentionOf(refused)).toEqual({ heading: 'The server is not running', message: 'port is already allocated' })
  })

  test("not running, with Docker's words for a container that stopped by itself", () => {
    expect(attentionOf(server({ state: 'stopped', status: 'Exited (2) 5 seconds ago' }))?.message).toBe(
      'Its container stopped: Exited (2) 5 seconds ago.'
    )
    expect(attentionOf(server({ state: 'absent', status: '', current: false }))?.message).toBe('Its container is gone.')
  })

  test('restarting again and again', () => {
    const looping = server({ state: 'restarting' })
    expect(serverTone(looping)).toBe('busy')
    expect(attentionOf(looping)?.heading).toBe('The server keeps restarting')
  })
})

describe('what is typed', () => {
  test('for the upstream servers is addresses apart by commas or spaces', () => {
    expect(upstreamsOf('1.1.1.1, 9.9.9.9')).toEqual(['1.1.1.1', '9.9.9.9'])
    expect(upstreamsOf('  1.1.1.1\n2606:4700:4700::1111 ,')).toEqual(['1.1.1.1', '2606:4700:4700::1111'])
    expect(upstreamsOf('  ')).toEqual([])
  })

  test('for the port is a port or nothing', () => {
    expect(portOf(' 53 ')).toBe(53)
    expect(portOf('65535')).toBe(65535)
    expect(portOf('0')).toBeUndefined()
    expect(portOf('65536')).toBeUndefined()
    expect(portOf('5e3')).toBeUndefined()
    expect(portOf('')).toBeUndefined()
  })
})

test('the records are counted in words', () => {
  expect(recordsSummary(0)).toBe('No names')
  expect(recordsSummary(1)).toBe('1 name')
  expect(recordsSummary(4)).toBe('4 names')
})
