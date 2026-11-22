/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { beforeEach, describe, expect, test } from 'bun:test'
import { appOf, setInstalledApps } from './apps.svelte'
import { BANNER_MS, MAX_BANNERS, Notifications } from './notifications.svelte'
import { WindowManager } from './windowManager.svelte'
import type { AppSummary } from '../shell/hostApi'

const summary = (id: string, extra: Partial<AppSummary> = {}): AppSummary => ({
  id,
  name: id[0]!.toUpperCase() + id.slice(1),
  version: '1.0.0',
  state: 'running',
  source: 'file',
  icon: 'clock',
  tint: 'teal',
  ...extra
})

/** A clock the test drives: `schedule` records, `tick` runs what is due. */
function clock() {
  let now = 0
  const pending: { at: number; run: () => void; live: boolean }[] = []
  return {
    schedule(run: () => void, ms: number) {
      const entry = { at: now + ms, run, live: true }
      pending.push(entry)
      return () => void (entry.live = false)
    },
    tick(ms: number) {
      now += ms
      for (const entry of pending) {
        if (entry.live && entry.at <= now) {
          entry.live = false
          entry.run()
        }
      }
    },
    live: () => pending.filter((p) => p.live).length
  }
}

let wm: WindowManager
let time: ReturnType<typeof clock>
let center: Notifications
const titles = () => center.banners.map((b) => b.title)

beforeEach(() => {
  setInstalledApps([summary('ticker'), summary('notes')])
  wm = new WindowManager()
  wm.bounds = { width: 1280, height: 800 }
  time = clock()
  center = new Notifications({ wm, appOf, schedule: time.schedule })
})

describe('what is shown', () => {
  test('the sender is named by the shell, from its own app list', () => {
    center.deliver({ app: 'ticker', title: 'Backup finished', body: '12 files' })
    expect(center.banners).toHaveLength(1)
    expect(center.banners[0]).toMatchObject({
      app: 'ticker',
      appName: 'Ticker',
      icon: 'clock',
      title: 'Backup finished',
      body: '12 files'
    })
  })

  test('with no window open for the sender, and while another app has focus, it is shown', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    wm.open('notes')
    center.deliver({ app: 'ticker', title: 'b' })
    expect(titles()).toEqual(['b', 'a'])
  })

  test('a sender that is not a running app here shows nothing', () => {
    center.deliver({ app: 'nobody', title: 'hello' })
    setInstalledApps([summary('ticker'), summary('notes', { state: 'disabled' })])
    center.deliver({ app: 'notes', title: 'hello' })
    expect(center.banners).toEqual([])
  })

  test('a message that is not a notification is dropped', () => {
    for (const bad of [
      null,
      'x',
      5,
      {},
      { app: 'ticker' },
      { app: 'ticker', title: 5 },
      { title: 't' },
      { app: 'ticker', title: 't', body: 1 }
    ]) {
      center.deliver(bad)
    }
    expect(center.banners).toEqual([])
  })

  test('the title is one cleaned line and the body keeps its lines; text of the app is never the sender', () => {
    center.deliver({ app: 'ticker', title: '‮Backup\nfinished​', body: 'a\n\n\n\nb‮' })
    expect(center.banners[0]).toMatchObject({
      appName: 'Ticker',
      title: 'Backup finished',
      body: 'a\n\nb'
    })
  })

  test('a title with nothing visible shows the app name, and an empty body is left out', () => {
    center.deliver({ app: 'ticker', title: '​​', body: '​' })
    expect(center.banners[0]?.title).toBe('Ticker')
    expect(center.banners[0]).not.toHaveProperty('body')
  })
})

describe('the focus rule', () => {
  test("hidden where one of the sender's windows has focus", () => {
    wm.open('ticker')
    center.deliver({ app: 'ticker', title: 'hidden' })
    expect(center.banners).toEqual([])
    center.deliver({ app: 'notes', title: 'shown' })
    expect(titles()).toEqual(['shown'])
  })

  test("shown once the sender's window has lost focus, and once it has been closed", () => {
    wm.open('ticker')
    wm.open('notes')
    center.deliver({ app: 'ticker', title: 'behind' })
    expect(titles()).toEqual(['behind'])
    wm.close(wm.windows.find((w) => w.appId === 'notes')!.id)
    wm.open('ticker')
    for (const win of [...wm.windows]) wm.close(win.id)
    center.deliver({ app: 'ticker', title: 'closed' })
    expect(titles()).toEqual(['closed', 'behind'])
  })

  test('a focused window that is minimized, or a hidden Slide Over, is not in front', () => {
    wm.open('ticker')
    const win = wm.windows[0]!
    wm.setLayout(win.id, 'slideover')
    wm.slideOverHidden = true
    center.deliver({ app: 'ticker', title: 'tucked away' })
    expect(titles()).toEqual(['tucked away'])
    wm.slideOverHidden = false
    center.deliver({ app: 'ticker', title: 'in view' })
    expect(titles()).toEqual(['tucked away'])
  })
})

describe('how many, and for how long', () => {
  test('at most three, newest first: a fourth takes the place of the oldest', () => {
    for (const n of ['1', '2', '3', '4']) center.deliver({ app: 'ticker', title: n })
    expect(MAX_BANNERS).toBe(3)
    expect(titles()).toEqual(['4', '3', '2'])
    expect(time.live()).toBe(3) // the pushed-out one's timer is gone with it
  })

  test('an alert goes by itself after the time, and not before', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    time.tick(BANNER_MS - 1)
    expect(titles()).toEqual(['a'])
    time.tick(1)
    expect(center.banners).toEqual([])
  })

  test('held while it is read, and the full time again once released', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    const id = center.banners[0]!.id
    center.hold(id)
    time.tick(BANNER_MS * 5)
    expect(titles()).toEqual(['a'])
    center.release(id)
    time.tick(BANNER_MS - 1)
    expect(titles()).toEqual(['a'])
    time.tick(1)
    expect(center.banners).toEqual([])
  })

  test('releasing an alert that is already gone brings nothing back', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    const id = center.banners[0]!.id
    center.dismiss(id)
    center.release(id)
    expect(center.banners).toEqual([])
    expect(time.live()).toBe(0)
  })
})

describe('acting on an alert', () => {
  test('dismiss closes it and opens nothing', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    center.dismiss(center.banners[0]!.id)
    expect(center.banners).toEqual([])
    expect(wm.windows).toEqual([])
    expect(time.live()).toBe(0)
  })

  test("select opens the sender's window, and closes the alert", () => {
    center.deliver({ app: 'ticker', title: 'a' })
    center.select(center.banners[0]!.id)
    expect(center.banners).toEqual([])
    expect(wm.windows.map((w) => w.appId)).toEqual(['ticker'])
    expect(wm.focused?.appId).toBe('ticker')
  })

  test('select brings an open window of the sender forward instead of opening a second', () => {
    wm.open('ticker')
    wm.open('notes')
    center.deliver({ app: 'ticker', title: 'a' })
    center.select(center.banners[0]!.id)
    expect(wm.windows.filter((w) => w.appId === 'ticker')).toHaveLength(1)
    expect(wm.focused?.appId).toBe('ticker')
  })

  test('an app that stops running takes its alerts away, and the others stay', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    center.deliver({ app: 'notes', title: 'b' })
    center.forgetApps(['ticker'])
    expect(titles()).toEqual(['b'])
    expect(time.live()).toBe(1)
  })
})
