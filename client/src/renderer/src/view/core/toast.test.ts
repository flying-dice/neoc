import { describe, expect, test } from 'bun:test'
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
