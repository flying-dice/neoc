import { describe, expect, test } from 'bun:test'
import { FakeTimers } from '../host/clock'
import { createHarness } from '../host/harness'
import type { ViewCommand } from '../protocol'

const wait = (ms = 20): Promise<void> => new Promise((r) => setTimeout(r, ms))

describe('toast with an action', () => {
  test('the host sends text, icon and an action ({title, id}); the notice keeps them', async () => {
    const h = createHarness({ latencyMs: 0 })
    const view = await h.openNotes()
    h.host.allViews()[0]!.command({
      cmd: 'toast',
      args: {
        text: 'Note deleted',
        kind: 'success',
        icon: 'trash',
        action: { title: 'Undo', id: 't1' },
        duration: 0
      }
    } as unknown as ViewCommand)
    await wait()
    expect(view.store.notices).toHaveLength(1)
    expect(view.store.notices[0]).toMatchObject({
      kind: 'success',
      text: 'Note deleted',
      icon: 'trash',
      action: { title: 'Undo', id: 't1' }
    })
  })

  test('tapping the action reports `tap` on `$toast/<id>` and dismisses the toast', async () => {
    const h = createHarness({ latencyMs: 0 })
    const view = await h.openNotes()
    h.host.allViews()[0]!.command({
      cmd: 'toast',
      args: { text: 'Gone', action: { title: 'Undo', id: 't7' }, duration: 0 }
    } as unknown as ViewCommand)
    await wait()
    const id = view.store.notices[0]!.id
    view.toastAction(id)
    await wait()
    expect(view.store.notices.some((n) => n.id === id)).toBe(false)
    const event = h.host.received.find((r) => r.method === 'view.event')
    expect(event?.params).toMatchObject({ event: 'tap', path: '$toast/t7' })
  })

  test('a toast without an action has none, and an unknown icon is still stored for the renderer to ignore', async () => {
    const h = createHarness({ latencyMs: 0 })
    const view = await h.openNotes()
    view.notice('info', 'Hello')
    expect(view.store.notices[0]).toEqual({ id: 1, kind: 'info', text: 'Hello' })
  })

  const toast = (text: string, extra: object = {}): ViewCommand =>
    ({ cmd: 'toast', args: { text, ...extra } }) as unknown as ViewCommand

  test('a toast with an action stays until it is answered, however long the person takes (WCAG 2.2.1)', async () => {
    const timers = new FakeTimers()
    const h = createHarness({ latencyMs: 0, timers })
    const view = await h.openNotes()
    h.host.allViews()[0]!.command(toast('Deleted', { action: { title: 'Undo', id: 't1' } }))
    h.host
      .allViews()[0]!
      .command(
        toast('Even with a duration', { action: { title: 'Undo', id: 't2' }, duration: 1000 })
      )
    await timers.advance(10 * 60_000)
    expect(view.store.notices.map((n) => n.text)).toEqual(['Deleted', 'Even with a duration'])
  })

  test('an Undo notice (a write replaced a change made elsewhere) stays too', async () => {
    const timers = new FakeTimers()
    const h = createHarness({ latencyMs: 0, timers })
    const view = await h.openNotes()
    view.notice('conflict', 'Your edit replaced a change', undefined, {
      undo: { source: 'note', key: 'n1', field: 'body', revisionId: 3 }
    })
    await timers.advance(10 * 60_000)
    expect(view.store.notices).toHaveLength(1)
  })

  test('a plain toast expires after six seconds, and a zero duration keeps it', async () => {
    const timers = new FakeTimers()
    const h = createHarness({ latencyMs: 0, timers })
    const view = await h.openNotes()
    view.notice('info', 'Saved')
    view.notice('info', 'Pinned', 0)
    await timers.advance(5_900)
    expect(view.store.notices).toHaveLength(2)
    await timers.advance(200)
    expect(view.store.notices.map((n) => n.text)).toEqual(['Pinned'])
  })

  test('a toast waits while the pointer or focus is on it, then gets its full time again', async () => {
    const timers = new FakeTimers()
    const h = createHarness({ latencyMs: 0, timers })
    const view = await h.openNotes()
    view.notice('info', 'Reading me')
    const id = view.store.notices[0]!.id
    await timers.advance(4_000)
    view.holdNotice(id)
    await timers.advance(60_000)
    expect(view.store.notices).toHaveLength(1)
    view.releaseNotice(id)
    await timers.advance(5_900)
    expect(view.store.notices).toHaveLength(1)
    await timers.advance(200)
    expect(view.store.notices).toHaveLength(0)
  })

  test('releasing a toast that is already gone, or never expired, does nothing', async () => {
    const h = createHarness({ latencyMs: 0 })
    const view = await h.openNotes()
    view.releaseNotice(99)
    view.holdNotice(99)
    view.notice('info', 'Pinned', 0)
    view.releaseNotice(view.store.notices[0]!.id)
    expect(view.store.notices).toHaveLength(1)
  })

  test('at most five notices show; the oldest plain one goes first and an unanswered Undo outlives it', async () => {
    const h = createHarness({ latencyMs: 0 })
    const view = await h.openNotes()
    h.host.allViews()[0]!.command(toast('Undo me', { action: { title: 'Undo', id: 'u' } }))
    await wait()
    for (let i = 1; i <= 5; i++) view.notice('info', `plain ${i}`, 0)
    await wait()
    expect(view.store.notices.map((n) => n.text)).toEqual([
      'Undo me',
      'plain 2',
      'plain 3',
      'plain 4',
      'plain 5'
    ])
  })

  test('view.command copy writes the clipboard only right after a tap or key press', async () => {
    const copied: string[] = []
    let active = false
    const errors: unknown[] = []
    const h = createHarness({ latencyMs: 0, onError: (e) => errors.push(e) })
    await h.openNotes({
      effects: { copy: (t) => void copied.push(t), userActive: () => active }
    })
    const command = { cmd: 'copy', args: { text: 'secret' } } as unknown as ViewCommand
    h.host.allViews()[0]!.command(command)
    await wait()
    expect(copied).toEqual([])
    expect(errors.some((e) => String(e).includes('copy ignored'))).toBe(true)
    active = true
    h.host.allViews()[0]!.command(command)
    await wait()
    expect(copied).toEqual(['secret'])
  })

  test('link opening and announcements go through the view', async () => {
    const h = createHarness({ latencyMs: 0 })
    const opened: string[] = []
    const view = await h.openNotes({ effects: { openUrl: (u) => void opened.push(u) } })
    view.openUrl('https://example.com')
    view.announce('Moved to Done')
    expect(opened).toEqual(['https://example.com'])
    expect(view.store.announcement).toBe('Moved to Done')
  })
})
