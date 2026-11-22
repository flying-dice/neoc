/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { afterEach, beforeEach, describe, expect, spyOn, test } from 'bun:test'
import { appOf, setInstalledApps } from './apps.svelte'
import {
  BANNER_MS,
  MAX_BANNERS,
  Notifications,
  type NotificationDeps
} from './notifications.svelte'
import { SPOKEN_BODY_MAX } from '../shell/notificationText'
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

/** The page the person is using: shown or hidden, in front or not; the test flips it and tells the alerts. */
function page() {
  const listeners = new Set<() => void>()
  const state = { visible: true, focused: true }
  return {
    state,
    deps: {
      pageVisible: () => state.visible,
      pageInFront: () => state.visible && state.focused,
      onPageVisibility: (listener: () => void) => {
        listeners.add(listener)
        return () => void listeners.delete(listener)
      }
    } satisfies Pick<NotificationDeps, 'pageVisible' | 'pageInFront' | 'onPageVisibility'>,
    show(visible: boolean) {
      state.visible = visible
      listeners.forEach((l) => l())
    }
  }
}

let wm: WindowManager
let time: ReturnType<typeof clock>
let tab: ReturnType<typeof page>
let center: Notifications
const titles = () => center.banners.map((b) => b.title)
let warn: ReturnType<typeof spyOn>

beforeEach(() => {
  setInstalledApps([summary('ticker'), summary('notes'), summary('alpha'), summary('beta')])
  wm = new WindowManager()
  wm.bounds = { width: 1280, height: 800 }
  time = clock()
  tab = page()
  center = new Notifications({ wm, appOf, schedule: time.schedule, ...tab.deps })
  warn = spyOn(console, 'warn').mockImplementation(() => {})
})

afterEach(() => warn.mockRestore())

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

  test('a sender that is not a running app here shows nothing, and the shell logs who it was', () => {
    center.deliver({ app: 'nobody', title: 'hello' })
    expect(warn).toHaveBeenCalledTimes(1)
    expect(String(warn.mock.calls[0]?.[0])).toContain('"nobody"')
    setInstalledApps([summary('ticker'), summary('notes', { state: 'disabled' })])
    center.deliver({ app: 'notes', title: 'hello' })
    expect(warn).toHaveBeenCalledTimes(2)
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
    center.deliver({
      app: 'ticker',
      title: '\u202EBackup\nfinished\u200B',
      body: 'a\n\n\n\nb\u202E'
    })
    expect(center.banners[0]).toMatchObject({
      appName: 'Ticker',
      title: 'Backup finished',
      body: 'a\n\nb'
    })
  })

  test('a title with nothing visible shows the app name, and an empty body is left out', () => {
    center.deliver({ app: 'ticker', title: '\u200B\u200B', body: '\u200B' })
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

  test("one noisy app competes with itself first: its burst does not push another app's alert off the screen", () => {
    center.deliver({ app: 'beta', title: 'B1' })
    for (const n of ['A1', 'A2', 'A3', 'A4']) center.deliver({ app: 'alpha', title: n })
    // Beta's single alert stays; Alpha's own oldest were replaced
    expect(titles()).toEqual(['A4', 'A3', 'B1'])
    expect(time.live()).toBe(3)
  })

  test('a sender with nothing on screen displaces the oldest of the others', () => {
    for (const n of ['A1', 'A2', 'A3']) center.deliver({ app: 'alpha', title: n })
    center.deliver({ app: 'beta', title: 'B1' })
    expect(titles()).toEqual(['B1', 'A3', 'A2'])
  })

  test('an alert someone is reading is the last to go', () => {
    for (const n of ['A1', 'A2', 'A3']) center.deliver({ app: 'alpha', title: n })
    const oldest = center.banners[2]!
    center.hold(oldest.id) // the keyboard is on it
    center.deliver({ app: 'alpha', title: 'A4' })
    expect(titles()).toEqual(['A4', 'A3', 'A1'])
    // and when every alert of the sender is held, the oldest free one of another app goes before them
    center.deliver({ app: 'beta', title: 'B1' })
    expect(titles()).toContain('A1')
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

describe('a page nobody is looking at', () => {
  test('a tab in the background, or a browser with another application in front, does not count as the sender being in front', () => {
    wm.open('ticker')
    tab.state.focused = false
    center.deliver({ app: 'ticker', title: 'shown anyway' })
    expect(titles()).toEqual(['shown anyway'])
    tab.state.focused = true
    center.deliver({ app: 'ticker', title: 'hidden again' })
    expect(titles()).toEqual(['shown anyway'])
    tab.show(false)
    center.deliver({ app: 'ticker', title: 'tab hidden' })
    expect(titles()).toEqual(['tab hidden', 'shown anyway'])
  })

  test('the time is counted only while the page is on screen: an alert that arrives in a hidden tab waits for it', () => {
    tab.show(false)
    center.deliver({ app: 'ticker', title: 'unseen' })
    time.tick(BANNER_MS * 10)
    expect(titles()).toEqual(['unseen'])
    expect(time.live()).toBe(0)
    tab.show(true)
    time.tick(BANNER_MS - 1)
    expect(titles()).toEqual(['unseen'])
    time.tick(1)
    expect(center.banners).toEqual([])
  })

  test('hiding the page stops the clock, showing it again gives the full time', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    time.tick(BANNER_MS - 1)
    tab.show(false)
    time.tick(BANNER_MS * 5)
    expect(titles()).toEqual(['a'])
    tab.show(true)
    time.tick(BANNER_MS - 1)
    expect(titles()).toEqual(['a'])
    time.tick(1)
    expect(center.banners).toEqual([])
  })

  test('an alert being read is not started again when the page is shown', () => {
    center.deliver({ app: 'ticker', title: 'a' })
    center.hold(center.banners[0]!.id)
    tab.show(false)
    tab.show(true)
    time.tick(BANNER_MS * 3)
    expect(titles()).toEqual(['a'])
  })
})

describe('what is told to a screen reader', () => {
  test('the sender, the title and the start of a body: a long body is clamped', () => {
    center.deliver({ app: 'ticker', title: 'Backup', body: 'x '.repeat(1000) })
    const { announcement, spokenBody } = center.banners[0]!
    expect(announcement.startsWith('Ticker: Backup. x x')).toBe(true)
    expect(Array.from(spokenBody ?? '').length).toBeLessThanOrEqual(SPOKEN_BODY_MAX + 1)
    expect(announcement.length).toBeLessThan(SPOKEN_BODY_MAX + 40)
  })

  test('a title that shows nothing is announced as the app name, and a flag and a family emoji survive', () => {
    center.deliver({ app: 'ticker', title: '\u2800\u2800' })
    expect(center.banners[0]?.announcement).toBe('Ticker: Ticker')
    center.deliver({
      app: 'ticker',
      title:
        '\u{1F468}\u200D\u{1F469}\u200D\u{1F467} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}'
    })
    expect(center.banners[0]?.title).toBe(
      '\u{1F468}\u200D\u{1F469}\u200D\u{1F467} \u{1F3F4}\u{E0067}\u{E0062}\u{E0065}\u{E006E}\u{E0067}\u{E007F}'
    )
  })
})
