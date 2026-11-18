/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { describe, expect, test } from 'bun:test'
import { FakeTimers } from '../host/clock'
import { createHarness, type HarnessOptions } from '../host/harness'
import type { Node, Op, RowKey } from '../protocol'
import { locate } from './tree'
import type { RowsData } from './store'

/** Client core + session + real transport + fake host, on a manual clock. */
async function setup(options: HarnessOptions = {}) {
  const timers = new FakeTimers()
  const errors: unknown[] = []
  const h = createHarness({ timers, onError: (e) => errors.push(e), ...options, latencyMs: 0 })
  const view = await h.openNotes()
  h.link.setLatency(options.latencyMs ?? 0) // latency applies after the handshake and the open
  const received = (method: string) => h.host.received.filter((r) => r.method === method)
  return { h, view, timers, errors, received, store: view.store }
}

const text = (v: Node | undefined) => v?.props?.text
const node = (view: { store: { tree: Node | null } }, path: string): Node =>
  locate(view.store.tree as Node, path)!.node
const rows = (view: { store: { sources: Record<string, unknown> } }, id = 'notes') =>
  view.store.sources[id] as RowsData

describe('view.open', () => {
  test('one round trip: tree, state, client state, source declarations and the first window', async () => {
    const { view, store, h } = await setup({ notes: { extra: 3 } })
    expect(h.host.received.map((r) => r.method).slice(0, 2)).toEqual(['session.hello', 'view.open'])
    expect(store.phase).toBe('ready')
    expect(store.link).toBe('online')
    expect(store.tree?.key).toBe('main')
    expect(store.client).toEqual({ search: '', noteId: 1 })
    expect(store.state.body).toBe('')
    expect(rows(view)).toMatchObject({ kind: 'rows', total: 5, from: 0 })
    expect(rows(view).keys).toEqual([5, 4, 3, 2, 1])
    expect(view.evaluate({ $fn: 'concat', args: [{ $ref: 'src.notes.total' }, ' notes'] })).toBe(
      '5 notes'
    )
    expect(view.resolve('src.note.body')).toBe('milk, eggs') // a single-row source parameterised by client.noteId
  })

  test('the handshake carries the version, locale and zone', async () => {
    const { h } = await setup()
    expect(h.host.received[0].params).toMatchObject({
      proto: 1,
      kit: '1.0',
      locale: 'en-GB',
      timeZone: 'UTC'
    })
    expect(h.session.host).toMatchObject({ proto: 1, hostVersion: 'fake-0.1', resumed: false })
  })
})

describe('keyed ops: structure, versioned', () => {
  test('an event that changes structure arrives as ops, before the response settles the event', async () => {
    const { view, h } = await setup()
    const res = await view.emit('main/_0/actions/delete-all', 'tap')
    expect(res).toEqual({ ok: true })
    expect(node(view, 'main/_0/status').props?.text).toBe('Deleted all notes') // the UI already reflects it
    expect(locate(view.store.tree as Node, 'main/_0/empty-state')).not.toBeNull()
    expect(view.store.tree!.children![0].children!.map((c) => c.key)).toEqual([
      'count',
      'list',
      'empty-state',
      'body',
      'note-body',
      'pinned',
      'approved',
      'actions',
      'status'
    ])
    expect(rows(view).total).toBe(0)
    h.app.agentAddNote(h.host, 'Back')
    await Promise.resolve()
    await new Promise((r) => setTimeout(r, 0))
    expect(locate(view.store.tree as Node, 'main/_0/empty-state')).toBeNull() // removed by key
  })

  test('ops carry base and rev: the client stays in step across batches', async () => {
    const { view } = await setup()
    const rev0 = view.store.rev
    await view.emit('main/_0/actions/save', 'tap') // empty draft: one status op
    await view.emit('main/_0/actions/save', 'tap')
    expect(view.store.rev).toBe(rev0 + 2)
  })

  test('a gap (base != rev) makes the client ask for the full tree, and nothing is guessed', async () => {
    const { view, timers, received } = await setup()
    view.notify('view.ops', {
      viewId: view.viewId,
      base: 99,
      rev: 100,
      ops: [{ op: 'set', path: 'main/_0/status', props: { text: 'lost' } }]
    })
    await timers.settle()
    expect(received('view.sync')).toHaveLength(1)
    expect(node(view, 'main/_0/status').props?.text).toBe('Ready') // the lost op was not applied
  })

  test('an op with a broken address also resyncs, and drafts survive the resync', async () => {
    const { view, timers, received } = await setup()
    view.write('state.body', 'half-typed')
    const ops: Op[] = [{ op: 'set', path: 'main/ghost', props: {} }]
    view.notify('view.ops', {
      viewId: view.viewId,
      base: view.store.rev,
      rev: view.store.rev + 1,
      ops
    })
    await timers.settle()
    expect(received('view.sync')).toHaveLength(1)
    expect(view.resolve('state.body')).toBe('half-typed')
  })

  test('several gaps in a row share one view.sync', async () => {
    const { view, timers, received } = await setup()
    for (let i = 0; i < 3; i++)
      view.notify('view.ops', { viewId: view.viewId, base: 50 + i, rev: 51 + i, ops: [] })
    await timers.settle()
    expect(received('view.sync')).toHaveLength(1)
  })

  test('view.tree replaces the tree; view.error keeps the last good tree and surfaces the message', async () => {
    const { view } = await setup()
    view.notify('view.error', { viewId: view.viewId, message: 'render failed' })
    expect(view.store.error).toBe('render failed')
    expect(view.store.tree).not.toBeNull()
    view.notify('view.tree', {
      viewId: view.viewId,
      rev: 9,
      tree: { type: 'Text', key: 'main', props: { text: 'fresh' } }
    })
    expect(view.store.rev).toBe(9)
    expect(view.store.error).toBeNull()
    expect(text(view.store.tree!)).toBe('fresh')
  })

  test('view.closed marks the window gone', async () => {
    const { view } = await setup()
    view.notify('view.closed', { viewId: view.viewId, reason: 'app crashed' })
    expect(view.store.phase).toBe('closed')
    expect(view.store.closed).toBe('app crashed')
  })
})

describe('host lifecycle notifications', () => {
  test('view.closed "reloaded" (a dev hot reload) reopens the same window, drafts kept', async () => {
    const { view, timers, received } = await setup()
    view.write('state.body', 'half typed')
    view.notify('view.closed', { viewId: view.viewId, reason: 'reloaded' })
    await timers.settle()
    expect(received('view.open')).toHaveLength(2)
    expect(received('view.open')[1].params).toMatchObject({
      resume: { drafts: [{ kind: 'state', path: 'body', value: 'half typed' }] }
    })
    expect(view.store.phase).toBe('ready')
    expect(view.store.closed).toBeNull()
    expect(view.resolve('state.body')).toBe('half typed')
  })

  test('data.patch declare registers a source a re-render started to use, before its first window', async () => {
    const { view } = await setup()
    view.notify('data.patch', {
      viewId: view.viewId,
      declare: { extra: { kind: 'rows', key: 'id', page: 10, params: {} } },
      sources: {
        extra: {
          kind: 'rows',
          total: 2,
          from: 0,
          keys: [1, 2],
          rows: { '1': { id: 1 }, '2': { id: 2 } }
        }
      }
    })
    expect(view.sources.declaration('extra')).toMatchObject({ kind: 'rows', page: 10 })
    expect(view.sources.page('extra')).toBe(10)
    expect(rows(view, 'extra')).toMatchObject({ total: 2, keys: [1, 2] })
    expect(view.sources.keyField('extra')).toBe('id')
  })
})

describe('two-way text binding: drafts, shielding, merge (spec 3.4)', () => {
  test('typing shows at once; the write-back is debounced into one state.set with baseRev', async () => {
    const { view, timers, received, h } = await setup()
    for (const v of ['h', 'he', 'hel', 'hell', 'hello']) view.write('state.body', v)
    expect(view.resolve('state.body')).toBe('hello') // the draft overlay, zero round trips
    expect(view.store.dirty['state:body']).toBe(true)
    expect(received('state.set')).toHaveLength(0)
    await timers.advance(299)
    expect(received('state.set')).toHaveLength(0)
    await timers.advance(1)
    expect(received('state.set')).toHaveLength(1)
    expect(received('state.set')[0].params).toMatchObject({
      path: 'body',
      value: 'hello',
      baseRev: 1
    })
    expect(h.host.stateValue('body')).toBe('hello')
    expect(view.store.dirty['state:body']).toBeUndefined()
    expect(view.store.drafts['state:body']).toBeUndefined()
    expect(view.resolve('state.body')).toBe('hello')
  })

  test('maxWait bounds how long continuous typing can defer the write', async () => {
    const { view, timers, received } = await setup()
    for (let i = 0; i < 30; i++) {
      view.write('state.body', 'x'.repeat(i + 1))
      await timers.advance(100) // faster than the 300 ms debounce, forever
    }
    expect(received('state.set').length).toBeGreaterThanOrEqual(2)
    expect(received('state.set')[0].params.value).toBe('x'.repeat(10)) // flushed at the 1000 ms maxWait
  })

  test('a remote change to a clean field applies immediately', async () => {
    const { view, timers, h } = await setup()
    h.app.agentAppendDraft(h.host, 'from agent')
    await timers.settle()
    expect(view.resolve('state.body')).toBe('from agent')
  })

  test('a dirty field shields itself; the host merges on the ack and the client adopts the result', async () => {
    const { view, timers, h } = await setup()
    view.write('state.body', 'user text')
    h.app.agentAppendDraft(h.host, '+agent')
    await timers.settle()
    expect(view.resolve('state.body')).toBe('user text') // never clobbered mid-typing
    expect(view.store.state.body).toBe('+agent') // the remote value is held behind the draft
    await timers.advance(300)
    expect(h.host.stateValue('body')).toBe('+agentuser text') // both pure inserts at 0: the agent's first
    expect(view.resolve('state.body')).toBe('+agentuser text')
    expect(view.store.dirty['state:body']).toBeUndefined()
    expect(view.store.announcement).toBe('Updated with a change made elsewhere')
    expect(view.store.notices).toHaveLength(0) // a non-overlapping merge needs no toast
  })

  test('the write carries the base the draft was typed on, not the held remote revision', async () => {
    const { view, timers, received, h } = await setup()
    view.write('state.body', 'mine')
    h.app.agentAppendDraft(h.host, 'theirs') // rev 2 while the draft is based on rev 1
    await timers.advance(300)
    expect(received('state.set')[0].params.baseRev).toBe(1)
  })

  test('overlapping edits resolve to the human and surface a conflict notice', async () => {
    const { view, timers, h } = await setup()
    h.host.setState('body', 'buy milk')
    await timers.settle()
    view.write('state.body', 'buy milk') // adopt the base
    view.write('state.body', 'buy soy')
    h.host.setState('body', 'buy tea') // an agent replaced the same words
    await timers.advance(300)
    expect(h.host.stateValue('body')).toBe('buy soy')
    expect(view.resolve('state.body')).toBe('buy soy')
    expect(view.store.notices.map((n) => [n.kind, n.text])).toEqual([
      ['conflict', 'Your edit replaced a change made elsewhere.']
    ])
    expect(h.host.overwritten).toEqual([{ path: 'body', theirs: 'buy tea', mine: 'buy soy' }]) // kept as a revision
  })

  test('typing in flight is rebased onto the ack: neither the merged content nor the new keystrokes are lost', async () => {
    const { view, timers, h } = await setup({ latencyMs: 50 })
    view.write('state.body', 'draft')
    h.host.setState('body', '+agent') // held behind the draft
    await timers.advance(300) // the write goes out (arrives after 50 ms, the ack 50 ms later)
    view.write('state.body', 'draft!') // typed while the write is in flight
    await timers.advance(200)
    await timers.advance(400)
    expect(h.host.stateValue('body')).toBe('+agentdraft!')
    expect(view.resolve('state.body')).toBe('+agentdraft!')
    expect(view.store.dirty['state:body']).toBeUndefined()
  })

  test('typing and deleting back to the host value sends nothing', async () => {
    const { view, timers, received } = await setup()
    view.write('state.body', 'a')
    view.write('state.body', '')
    await timers.advance(1000)
    expect(received('state.set')).toHaveLength(0)
    expect(view.store.dirty['state:body']).toBeUndefined()
  })

  test('blur and submit write now; IME composition never flushes mid-composition', async () => {
    const { view, timers, received } = await setup()
    const b = view.bindingOf({ $ref: 'state.body' })!
    b.setComposing(true)
    b.set('こ', view.syncPolicy('change'))
    await timers.advance(1000)
    expect(received('state.set')).toHaveLength(0) // composing: nothing leaves
    b.setComposing(false)
    await timers.advance(0)
    expect(received('state.set')).toHaveLength(1)
    b.set('こん', view.syncPolicy('blur'))
    await timers.advance(1000)
    expect(received('state.set')).toHaveLength(1) // blur mode: only an explicit commit writes
    await b.commit()
    expect(received('state.set')).toHaveLength(2)
  })

  test('a refused write keeps the draft and shows the field error, without a retry loop', async () => {
    const { view, timers, errors } = await setup()
    view.write('state.nope', 'x') // the host has no such state path: it answers with an error
    await timers.advance(300)
    await timers.advance(2000)
    expect(view.store.fieldErrors['state:nope']).toBe('Could not be saved')
    expect(view.resolve('state.nope')).toBe('x')
    expect(errors.length).toBeGreaterThan(0)
  })
})

describe('scalar bindings: human wins; strict sources refuse', () => {
  test('a scalar write goes out at once and the human value stands over a change made elsewhere', async () => {
    const { view, timers, h } = await setup()
    h.host.setState('approved', false) // rev 2, unseen by a write based on rev 1... the view saw it clean
    await timers.settle()
    view.write('state.approved', true)
    await timers.advance(0)
    expect(h.host.stateValue('approved')).toBe(true)
    expect(view.resolve('state.approved')).toBe(true)
  })

  test('a toggle in a list row is a row.set with the row revision as baseRev', async () => {
    const { view, timers, received, h } = await setup()
    const scope = { row: rows(view).rows['2'], rowKey: 2 as RowKey, source: 'notes' }
    view.write('row.pinned', true, scope)
    expect(view.resolve('row.pinned', scope)).toBe(true) // optimistic overlay
    await timers.advance(0)
    expect(received('row.set')[0].params).toMatchObject({
      source: 'notes',
      key: 2,
      field: 'pinned',
      value: true,
      baseRev: 1
    })
    expect(h.app.notes.find((n) => n.id === 2)!.pinned).toBe(true)
    expect(rows(view).rows['2'].pinned).toBe(true)
    expect(view.store.dirty['row:notes:2:pinned']).toBeUndefined()
  })

  test('a strict source refuses a stale write: adopt the server value, announce, error state', async () => {
    const { view, timers, h } = await setup({ notes: { approvedMerge: 'reject' } })
    h.host.setState('approved', true) // rev 2: the client sees it, then...
    await timers.settle()
    // ...a stale write: the client typed on rev 2 but the host moves on before the write lands.
    view.write('state.approved', false, undefined, view.syncPolicy('blur'))
    h.host.setState('approved', true) // rev 3 behind the draft
    h.host.setState('approved', true)
    await view.bindings.settle()
    expect(h.host.stateValue('approved')).toBe(true) // not overwritten
    expect(view.resolve('state.approved')).toBe(true) // the server value, adopted
    expect(view.store.fieldErrors['state:approved']).toBe('Changed elsewhere')
    expect(view.store.announcement).toContain('Changed elsewhere')
  })
})

describe('row bindings: src.<id>.<field> writes through row.set', () => {
  test('the selected note body is two-way: draft, debounced row.set, merge with an agent append', async () => {
    const { view, timers, received, h } = await setup()
    view.write('src.note.body', 'Milk, eggs')
    h.app.agentAppendNote(h.host, 1, '\n- oat milk') // rev 2 while the draft is based on rev 1
    await timers.settle()
    expect(view.resolve('src.note.body')).toBe('Milk, eggs') // shielded
    await timers.advance(300)
    expect(received('row.set')[0].params).toMatchObject({
      source: 'note',
      key: 1,
      field: 'body',
      baseRev: 1
    })
    expect(h.app.notes[0].body).toBe('Milk, eggs\n- oat milk') // merged on the base the host had sent
    expect(view.resolve('src.note.body')).toBe('Milk, eggs\n- oat milk')
  })

  test('a row write that replaced an agent change offers Undo; row.restore brings the lost value back', async () => {
    const { view, timers, received, h } = await setup()
    view.write('src.note.body', 'soy, eggs')
    h.app.notes[0].body = 'tea, eggs' // an agent rewrote the same words
    h.app.notes[0].rev++
    h.host.refreshSources()
    await timers.settle()
    await timers.advance(300)
    expect(h.app.notes[0].body).toBe('soy, eggs') // the human wins
    const notice = view.store.notices.find((n) => n.kind === 'conflict')!
    expect(notice.undo).toMatchObject({ source: 'note', key: 1, field: 'body' })
    const done = view.undo(notice.id)
    await timers.settle()
    await done
    expect(received('row.restore')).toHaveLength(1)
    expect(received('row.restore')[0].params).toMatchObject({ source: 'note', key: 1 })
    expect(h.app.notes[0].body).toBe('tea, eggs')
    expect(view.resolve('src.note.body')).toBe('tea, eggs')
    expect(view.store.notices.some((n) => n.id === notice.id)).toBe(false)
  })

  test('only writable fields are bindable', async () => {
    const { view } = await setup()
    expect(view.target('src.note.body')).toEqual({
      kind: 'row',
      source: 'note',
      key: 1,
      field: 'body'
    })
    expect(view.target('src.note.rev')).toBeNull()
    expect(view.target('env.now')).toBeNull()
    expect(view.bindingOf({ $ref: 'route.id' })).toBeNull()
    expect(() => view.write('env.now', 1)).toThrow('not writable')
  })
})

describe('sources: params are refs, windows, row deltas', () => {
  test('a client-state write is local; only the source params reach the host, once, debounced', async () => {
    const { view, timers, received, store } = await setup()
    view.write('client.search', 'g')
    view.write('client.search', 'gr')
    view.write('client.search', 'gro')
    expect(store.client.search).toBe('gro')
    expect(received('source.set')).toHaveLength(0)
    await timers.advance(149)
    expect(received('source.set')).toHaveLength(0)
    await timers.advance(1)
    expect(received('source.set')).toHaveLength(1)
    expect(received('source.set')[0].params).toMatchObject({
      source: 'notes',
      params: { q: 'gro' },
      range: { from: 0, to: 60 }
    })
    expect(rows(view)).toMatchObject({ total: 1, keys: [1] })
    expect(rows(view).rows).not.toHaveProperty('2')
  })

  test('a client action sets state with no emit, and the source follows', async () => {
    const { view, timers, received } = await setup()
    view.write('client.search', 'zzz')
    await timers.advance(150)
    expect(rows(view).total).toBe(0)
    const clear = node(view, 'main/_0/actions/clear')
    view.fire(clear, 'main/_0/actions/clear', 'tap')
    expect(view.store.client.search).toBe('')
    expect(received('view.event')).toHaveLength(0)
    await timers.advance(150)
    expect(rows(view).total).toBe(2)
  })

  test('stale replies are dropped by sequence number', async () => {
    const { view, timers, store } = await setup({ latencyMs: 40 })
    view.write('client.search', 'gro')
    await timers.advance(150) // request 1 in flight
    view.write('client.search', 'ide')
    await timers.advance(150) // request 2 sent while 1 may still be unanswered
    await timers.advance(500)
    expect(store.client.search).toBe('ide')
    expect(rows(view).keys).toEqual([2])
  })

  test('row deltas: only changed rows travel, removed rows are dropped from the cache', async () => {
    const { view, timers, h } = await setup()
    const before = JSON.stringify(rows(view).rows)
    h.app.agentAddNote(h.host, 'New')
    await timers.settle()
    expect(rows(view).total).toBe(3)
    expect(rows(view).keys[0]).toBe(3)
    const patches = h.link.log.filter(
      (m) => m.dir === 'h2c' && m.text.includes('data.patch') && m.text.includes('"notes"')
    )
    const last = JSON.parse(patches[patches.length - 1].text).params.sources.notes
    expect(Object.keys(last.rows)).toEqual(['3']) // the two unchanged rows were not re-sent
    expect(JSON.stringify(rows(view).rows)).not.toBe(before)
    await view.emit('main/_0/actions/delete-all', 'tap')
    expect(rows(view).rows).toEqual({})
  })

  test('windows: loadMore widens the range one page; the host sends keys for the window and only new rows', async () => {
    const { view, timers, received, h } = await setup({ notes: { extra: 98, page: 20 } }) // 100 notes, page 20
    expect(rows(view)).toMatchObject({ total: 100, from: 0 })
    expect(rows(view).keys).toHaveLength(20)
    expect(view.sources.hasMore('notes')).toBe(true)
    view.sources.loadMore('notes')
    view.sources.loadMore('notes') // coalesced to one frame
    await timers.advance(16)
    expect(received('source.set')).toHaveLength(1)
    expect(received('source.set')[0].params.range).toEqual({ from: 0, to: 40 })
    expect(rows(view).keys).toHaveLength(40)
    // The second reply carried only the 20 rows the client did not have.
    const reply = h.link.log.filter(
      (m) => m.dir === 'h2c' && m.text.includes('"result"') && m.text.includes('"keys"')
    )
    expect(Object.keys(JSON.parse(reply[reply.length - 1].text).result.rows)).toHaveLength(20)
  })

  test('setRange jumps to a window; rows far outside it are evicted past the cache cap', async () => {
    const { view, timers } = await setup({ notes: { extra: 398, page: 10 } }) // 400 notes, page 10: cap 50 rows
    for (let i = 1; i <= 8; i++) {
      view.sources.setRange('notes', i * 10, i * 10 + 10)
      await timers.advance(16)
    }
    const data = rows(view)
    expect(data.from).toBe(80)
    expect(data.keys).toHaveLength(10)
    expect(Object.keys(data.rows).length).toBeLessThanOrEqual(50)
    for (const k of data.keys) expect(data.rows).toHaveProperty(String(k)) // the window itself is always resident
  })

  test('a new query starts back at the top of the list', async () => {
    const { view, timers, received } = await setup({ notes: { extra: 98, page: 20 } })
    view.sources.loadMore('notes')
    await timers.advance(16)
    view.write('client.search', 'Generated')
    await timers.advance(150)
    expect(received('source.set').at(-1)!.params.range).toEqual({ from: 0, to: 20 })
  })
})

describe('client-managed state (spec 3.5)', () => {
  test('persist=session survives a reload (a new view for the same window id reads it back)', async () => {
    const first = await setup()
    first.view.write('client.search', 'gro')
    const storage = first.h.storage
    const second = await setup({ storage })
    expect(second.store.client.search).toBe('gro')
  })

  test('persist=app comes back from the host snapshot, not from this client', async () => {
    const { store } = await setup()
    expect(store.client.noteId).toBe(1)
  })

  test('synced entries are mirrored to the host (client.state), not the ones declared none', async () => {
    const { view, timers, received } = await setup()
    view.write('client.noteId', 2)
    expect(received('client.state')).toHaveLength(0) // commit mirror: coalesced
    await timers.advance(150)
    expect(received('client.state')).toHaveLength(1)
    expect(received('client.state')[0].params).toMatchObject({ set: { noteId: 2 } })
  })

  test('selecting another note re-queries the single-row source through its param ref', async () => {
    const { view, timers } = await setup()
    view.write('client.noteId', 2)
    await timers.advance(250)
    expect(view.resolve('src.note.body')).toBe('server-driven UI')
  })

  test('the app drives client state with client.set, except over a focused text control', async () => {
    const { view } = await setup()
    view.notify('client.set', { viewId: view.viewId, path: 'noteId', value: 2 })
    expect(view.store.client.noteId).toBe(2)
    const b = view.bindingOf({ $ref: 'client.search' })!
    b.setFocused(true)
    b.set('typing')
    view.notify('client.set', { viewId: view.viewId, path: 'search', value: 'from app' })
    expect(view.store.client.search).toBe('typing') // the user's focused field wins
    view.notify('client.set', { viewId: view.viewId, path: 'search', value: 'forced', force: true })
    expect(view.store.client.search).toBe('forced')
  })
})

describe('events and actions (spec 2.4, 3.5)', () => {
  test('an event is addressed by key path and row key, and flushes dirty drafts first', async () => {
    const { view, received, h } = await setup()
    view.write('state.body', 'Buy milk')
    await view.emit('main/_0/actions/save', 'tap')
    const order = h.host.received
      .map((r) => r.method)
      .filter((m) => m === 'state.set' || m === 'view.event')
    expect(order).toEqual(['state.set', 'view.event']) // the app sees the edit before the event
    expect(received('view.event')[0].params).toMatchObject({
      path: 'main/_0/actions/save',
      event: 'tap',
      seq: 1,
      viewId: view.viewId
    })
    expect(h.app.notes.at(-1)!.title).toBe('Buy milk')
    expect(view.resolve('state.body')).toBe('') // the host cleared it: a server-driven change on a clean field
  })

  test('seq is monotonic per view', async () => {
    const { view, received } = await setup()
    await view.emit('main/_0/actions/save', 'tap')
    await view.emit('main/_0/actions/save', 'tap')
    expect(received('view.event').map((r) => r.params.seq)).toEqual([1, 2])
  })

  test('a handler error shows as a toast with the safe message', async () => {
    const { view } = await setup()
    const res = await view.emit('main/_0/actions/nope', 'tap')
    expect(res).toMatchObject({ ok: false, reason: 'error' })
    expect(view.store.notices.at(-1)).toMatchObject({ kind: 'error' })
    expect(view.store.notices.at(-1)!.text).toContain('unhandled event tap')
  })

  test('toasts expire', async () => {
    const { view, timers } = await setup()
    view.notice('info', 'hello')
    expect(view.store.notices).toHaveLength(1)
    await timers.advance(6000)
    expect(view.store.notices).toHaveLength(0)
  })

  test('optimistic client effects are restored when the event fails, unless a newer value replaced them', async () => {
    const { view } = await setup()
    const open: Node = {
      type: 'Button',
      key: 'x',
      on: {
        tap: [
          { do: 'set', ref: 'client.search', value: 'optimistic' },
          { do: 'emit', event: 'boom' }
        ]
      }
    }
    view.fire(open, 'main/_0/actions/nope', 'tap')
    expect(view.store.client.search).toBe('optimistic') // instant
    await new Promise((r) => setTimeout(r, 5))
    expect(view.store.client.search).toBe('') // the event failed: back to the before value
    view.fire(open, 'main/_0/actions/nope', 'tap')
    view.write('client.search', 'newer')
    await new Promise((r) => setTimeout(r, 5))
    expect(view.store.client.search).toBe('newer') // a newer value is not clobbered
  })

  test('toggle, seq and copy run on the client with no round trip', async () => {
    const copied: string[] = []
    const timers = new FakeTimers()
    const h = createHarness({ timers })
    const view = await h.openNotes({ effects: { copy: (t) => copied.push(t) } })
    const n: Node = {
      type: 'Button',
      key: 'x',
      on: {
        tap: [
          {
            do: 'seq',
            actions: [
              { do: 'toggle', ref: 'client.flag' },
              { do: 'copy', text: { $fn: 'concat', args: ['id ', { $ref: 'row.id' }] } }
            ]
          }
        ]
      }
    }
    view.store.client.flag = false
    view.fire(n, 'main/x', 'tap', { row: { id: 9 }, rowKey: 9 })
    expect(view.store.client.flag).toBe(true)
    expect(copied).toEqual(['id 9'])
    expect(h.host.received.filter((r) => r.method === 'view.event')).toHaveLength(0)
  })

  test('view.command: toast, announce, focus by key', async () => {
    const focused: string[] = []
    const h = createHarness({ timers: new FakeTimers() })
    const view = await h.openNotes({ effects: { focus: (p) => focused.push(p) } })
    h.host.allViews()[0].command({ cmd: 'toast', args: { text: 'Saved', kind: 'success' } })
    h.host.allViews()[0].command({ cmd: 'announce', args: { text: 'Note updated' } })
    h.host.allViews()[0].command({ cmd: 'focus', args: { key: 'note-body' } })
    await new Promise((r) => setTimeout(r, 5))
    expect(view.store.notices.at(-1)).toMatchObject({ kind: 'success', text: 'Saved' })
    expect(view.store.announcement).toBe('Note updated')
    expect(focused).toEqual(['main/_0/note-body'])
  })

  test('closing a window flushes drafts first, then closes the instance', async () => {
    const { view, h, received } = await setup()
    view.write('state.body', 'unsaved')
    await h.session.closeView(view)
    expect(h.host.received.map((r) => r.method).slice(-2)).toEqual(['state.set', 'view.close'])
    expect(h.host.stateValue('body')).toBe('unsaved')
    expect(received('view.close')).toHaveLength(1)
    expect(view.store.phase).toBe('closed')
  })
})
