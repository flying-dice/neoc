/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import {
  Bindings,
  classifyAck,
  parseSync,
  SCALAR_SYNC,
  TEXT_SYNC,
  type BindingsDeps,
  type WriteAck
} from './bindings'
import { bindingKey, plainStore, type BindingTarget } from './store'
import { FakeTimers } from '../host/clock'
import { RpcError, RpcErrorCode } from '../rpc/peer'

describe('sync policy', () => {
  test('the prop string maps to a policy; garbage falls back', () => {
    expect(parseSync('debounce:400', TEXT_SYNC)).toEqual({
      mode: 'debounce',
      debounceMs: 400,
      maxWaitMs: 1000
    })
    expect(parseSync('debounce:2500', TEXT_SYNC)).toMatchObject({
      debounceMs: 2500,
      maxWaitMs: 2500
    })
    expect(parseSync('change', TEXT_SYNC)).toEqual({ mode: 'change' })
    expect(parseSync('blur', TEXT_SYNC)).toEqual({ mode: 'blur' })
    expect(parseSync('submit', TEXT_SYNC)).toEqual({ mode: 'submit' })
    expect(parseSync('debounce:abc', TEXT_SYNC)).toMatchObject({ debounceMs: 300 })
    expect(parseSync('sometimes', SCALAR_SYNC)).toBe(SCALAR_SYNC)
    expect(parseSync(undefined, TEXT_SYNC)).toBe(TEXT_SYNC)
  })
})

describe('how the ack is read', () => {
  test('accepted, merged, replaced and rejected', () => {
    expect(classifyAck('a', { rev: 2, value: 'a' })).toBe('accepted')
    expect(classifyAck('a', { rev: 2, value: 'a\nb' })).toBe('merged')
    expect(classifyAck('a', { rev: 2, value: 'a', conflict: true })).toBe('replaced')
    expect(classifyAck('a', { rev: 2, value: 'b', conflict: true })).toBe('rejected')
    expect(classifyAck(true, { rev: 2, value: true })).toBe('accepted')
    expect(classifyAck({ x: 1 }, { rev: 2, value: { x: 1 } })).toBe('accepted')
  })
})

/** The state machine alone, with a scripted host: no session, no tree. */
function machine(initial = 'base') {
  const timers = new FakeTimers()
  const store = plainStore()
  const target: BindingTarget = { kind: 'state', path: 'body' }
  const key = bindingKey(target)
  store.state.body = initial
  let revision = 1
  let online = true
  const sent: { value: unknown; baseRev: number }[] = []
  let reply: (value: unknown, baseRev: number) => WriteAck | Promise<WriteAck> = (value) => ({
    rev: ++revision,
    value: value as string
  })
  const notices: string[] = []
  const deps: BindingsDeps = {
    store,
    timers,
    online: () => online,
    send: async (_t, value, baseRev) => {
      sent.push({ value, baseRev })
      return reply(value, baseRev)
    },
    read: () => ({ value: store.state.body, rev: revision }),
    adopt: (_t, value, rev) => {
      store.state.body = value
      revision = Math.max(revision, rev)
    },
    notify: (_k, text) => void notices.push(text),
    announce: (text) => void (store.announcement = text),
    onError: () => undefined
  }
  const bindings = new Bindings(deps)
  return {
    timers,
    store,
    bindings,
    target,
    key,
    sent,
    notices,
    setOnline: (v: boolean) => void (online = v),
    reply: (fn: typeof reply) => void (reply = fn)
  }
}

describe('drafts and write-back', () => {
  test('a draft overlays at once; the debounce collapses edits; the ack clears the draft', async () => {
    const m = machine()
    m.bindings.edit(m.target, 'a', TEXT_SYNC)
    m.bindings.edit(m.target, 'ab', TEXT_SYNC)
    expect(m.store.drafts[m.key]).toBe('ab')
    expect(m.store.state.body).toBe('base') // the server value is untouched
    await m.timers.advance(300)
    expect(m.sent).toEqual([{ value: 'ab', baseRev: 1 }])
    expect(m.store.drafts[m.key]).toBeUndefined()
    expect(m.store.state.body).toBe('ab')
  })

  test('submit and blur policies never time out', async () => {
    const m = machine()
    m.bindings.edit(m.target, 'x', { mode: 'submit' })
    await m.timers.advance(60_000)
    expect(m.sent).toHaveLength(0)
    await m.bindings.commit(m.target)
    expect(m.sent).toHaveLength(1)
  })

  test('offline queues: nothing is sent, the draft stays dirty and replays when told to', async () => {
    const m = machine()
    m.setOnline(false)
    m.bindings.edit(m.target, 'queued', { mode: 'change' })
    await m.timers.advance(5000)
    expect(m.sent).toHaveLength(0)
    expect(m.bindings.pending()).toEqual([{ kind: 'state', path: 'body', baseRev: 1 }])
    m.setOnline(true)
    await m.bindings.replay()
    expect(m.sent).toEqual([{ value: 'queued', baseRev: 1 }])
    expect(m.bindings.pending()).toEqual([])
  })

  test('a connection failure keeps the draft (and no error); a refusal marks the field', async () => {
    const m = machine()
    m.reply(() => Promise.reject(new RpcError(RpcErrorCode.ConnectionClosed, 'gone')))
    m.bindings.edit(m.target, 'x', { mode: 'change' })
    await m.timers.settle()
    expect(m.store.dirty[m.key]).toBe(true)
    expect(m.store.fieldErrors[m.key]).toBeUndefined()
    m.reply(() => Promise.reject(new RpcError(-32010, 'No')))
    await m.bindings.flush(m.key)
    expect(m.store.fieldErrors[m.key]).toBe('Could not be saved')
    expect(m.store.dirty[m.key]).toBe(true)
    m.bindings.edit(m.target, 'xy', { mode: 'blur' }) // editing again clears the error
    expect(m.store.fieldErrors[m.key]).toBeUndefined()
  })
})

describe('shielding and the base revision', () => {
  test('a remote change behind a dirty draft updates the server value but not the base', async () => {
    const m = machine()
    m.bindings.edit(m.target, 'mine', { mode: 'blur' })
    m.bindings.remote(m.target, 'theirs', 2)
    expect(m.store.state.body).toBe('theirs')
    expect(m.store.drafts[m.key]).toBe('mine') // shielded
    await m.bindings.commit(m.target)
    expect(m.sent[0].baseRev).toBe(1) // the base is what the draft was typed on
  })

  test('a stale or echoed remote value on a clean field is ignored', () => {
    const m = machine()
    m.bindings.remote(m.target, 'fresh', 3)
    m.bindings.remote(m.target, 'older', 2)
    m.bindings.remote(m.target, 'older', 3)
    expect(m.store.state.body).toBe('fresh')
  })

  test('a remote change newer than the ack survives it', async () => {
    const m = machine()
    m.reply(async (value) => {
      m.bindings.remote(m.target, 'newer remote', 10) // lands while the write is in flight
      return { rev: 5, value: value as string }
    })
    m.bindings.edit(m.target, 'mine', { mode: 'change' })
    await m.timers.settle()
    expect(m.store.drafts[m.key]).toBeUndefined()
    expect(m.store.state.body).toBe('newer remote')
    m.reply((value) => ({ rev: 11, value: value as string }))
    m.bindings.edit(m.target, 'next', { mode: 'change' })
    await m.timers.settle()
    expect(m.sent.at(-1)).toEqual({ value: 'next', baseRev: 10 }) // based on what the person was looking at
  })

  test('editing back to the host value cancels the write', async () => {
    const m = machine('same')
    m.bindings.edit(m.target, 'changed', TEXT_SYNC)
    m.bindings.edit(m.target, 'same', TEXT_SYNC)
    await m.timers.advance(2000)
    expect(m.sent).toHaveLength(0)
  })
})

describe('what the ack does to the screen', () => {
  test('a merged ack is adopted, announced, and tells the caret hook what changed', async () => {
    const m = machine('')
    const changes: unknown[] = []
    m.bindings.onAdopt(m.target, (c) => changes.push(c))
    m.reply((value) => ({ rev: 2, value: `agent: ${String(value)}` }))
    m.bindings.edit(m.target, 'typed', { mode: 'change' })
    await m.timers.settle()
    expect(m.store.state.body).toBe('agent: typed')
    expect(m.store.announcement).toBe('Updated with a change made elsewhere')
    expect(changes).toEqual([{ before: 'typed', after: 'agent: typed' }])
  })

  test('an overlap that replaced a change made elsewhere raises a conflict notice, not an announcement', async () => {
    const m = machine('')
    m.reply((value) => ({ rev: 2, value: value as string, conflict: true }))
    m.bindings.edit(m.target, 'mine', { mode: 'change' })
    await m.timers.settle()
    expect(m.notices).toEqual(['Your edit replaced a change made elsewhere.'])
  })

  test('a refusal adopts the server value and puts the field in its error state', async () => {
    const m = machine('server')
    m.reply(() => ({ rev: 4, value: 'server', conflict: true }))
    m.bindings.edit(m.target, 'mine', { mode: 'change' })
    await m.timers.settle()
    expect(m.store.state.body).toBe('server')
    expect(m.store.drafts[m.key]).toBeUndefined()
    expect(m.store.fieldErrors[m.key]).toBe('Changed elsewhere')
    expect(m.store.announcement).toContain('Changed elsewhere')
  })

  test('typing during the flight rebases the draft onto the merged ack', async () => {
    const m = machine('')
    let release!: () => void
    m.reply(
      (value) =>
        new Promise<WriteAck>((resolve) => {
          release = () => resolve({ rev: 2, value: `A:${String(value)}` })
        })
    )
    m.bindings.edit(m.target, 'ab', { mode: 'change' })
    await m.timers.settle()
    m.bindings.edit(m.target, 'abc', { mode: 'blur' }) // typed while 'ab' was in flight
    m.reply((value) => ({ rev: 3, value: value as string }))
    release()
    await m.timers.settle()
    expect(m.store.drafts[m.key]).toBe('A:abc') // the host's merged content and the new keystroke, both kept
    expect(m.store.dirty[m.key]).toBe(true)
  })

  test('IME composition holds the write until it ends', async () => {
    const m = machine()
    m.bindings.setComposing(m.target, true)
    m.bindings.edit(m.target, 'こん', { mode: 'change' })
    await m.timers.advance(2000)
    expect(m.sent).toHaveLength(0)
    m.bindings.setComposing(m.target, false)
    await m.timers.settle()
    expect(m.sent).toEqual([{ value: 'こん', baseRev: 1 }])
  })
})
