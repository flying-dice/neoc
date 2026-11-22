/* eslint-disable @typescript-eslint/explicit-function-return-type -- test helpers infer their return types */
import { beforeEach, describe, expect, test } from 'bun:test'
import { setInstalledApps } from './apps.svelte'
import { WindowManager } from './windowManager.svelte'
import type { AppSummary } from '../shell/hostApi'

const summary = (id: string, extra: Partial<AppSummary> = {}): AppSummary => ({
  id,
  name: id,
  version: '1.0.0',
  state: 'running',
  source: 'file',
  client: false,
  permissions: [],
  ...extra
})

let wm: WindowManager
beforeEach(() => {
  setInstalledApps([
    summary('calculator', { window: { w: 340, h: 560, minW: 320, minH: 500 } }),
    summary('notes')
  ])
  wm = new WindowManager()
  wm.bounds = { width: 1280, height: 800 }
})

const win = (appId: string) => wm.windows.find((w) => w.appId === appId)!

describe('opening', () => {
  test('an id that is not an installed, running app opens nothing and throws nothing (Blocker 8)', () => {
    wm.open('agents') // the Agents app is not installed on this host
    wm.open('constructor')
    wm.open('__proto__')
    wm.open('')
    expect(wm.windows).toEqual([])
    expect(wm.stages).toEqual([])
    expect(wm.activeStageId).toBeNull()
  })

  test('the same id opens once the app is installed, and stops opening once it is disabled', () => {
    wm.open('agents')
    expect(wm.windows).toHaveLength(0)
    setInstalledApps([summary('agents')])
    wm.open('agents')
    expect(wm.windows.map((w) => w.appId)).toEqual(['agents'])
    wm.close(win('agents').id)
    setInstalledApps([summary('agents', { state: 'disabled' })])
    wm.open('agents')
    expect(wm.windows).toHaveLength(0)
  })

  test('a built-in page opens without any host app', () => {
    setInstalledApps([])
    wm.open('files')
    expect(wm.windows.map((w) => w.appId)).toEqual(['files'])
  })

  test('opening an app that is already open focuses its window instead of adding another', () => {
    wm.open('calculator')
    wm.open('notes')
    wm.open('calculator')
    expect(wm.windows).toHaveLength(2)
    expect(wm.focused?.appId).toBe('calculator')
    expect(wm.activeStage?.windowIds).toContain(win('calculator').id)
  })

  test('each app opens as a stage of its own; the earlier one joins the strip', () => {
    wm.open('calculator')
    wm.open('notes')
    expect(wm.stages).toHaveLength(2)
    expect(wm.onStage.map((w) => w.appId)).toEqual(['notes'])
    expect(wm.recentStages.map((s) => s.apps.map((a) => a.id))).toEqual([['calculator']])
  })

  test('alongside joins the current stage, beside what is there', () => {
    wm.open('calculator')
    wm.open('notes', { alongside: true })
    expect(wm.stages).toHaveLength(1)
    expect(wm.onStage.map((w) => w.appId).sort()).toEqual(['calculator', 'notes'])
    expect(win('notes').frame.x).toBeGreaterThan(win('calculator').frame.x)
  })

  test('the window kind and params travel with the window', () => {
    wm.open('notes', { kind: 'editor', params: { id: 4 } })
    expect(win('notes')).toMatchObject({ kind: 'editor', params: { id: 4 } })
  })

  test('a window is never smaller than the app’s minimum, however small the screen', () => {
    wm.bounds = { width: 400, height: 300 }
    wm.open('calculator')
    const { width, height } = win('calculator').frame
    expect(width).toBeGreaterThanOrEqual(320)
    expect(height).toBeGreaterThanOrEqual(500)
  })
})

describe('frames', () => {
  test('setFrame keeps a floating window on screen, clear of the Dock, and not below the minimum', () => {
    wm.open('calculator')
    const id = win('calculator').id
    wm.setFrame(id, { x: 1200, y: 780, width: 900, height: 900 })
    // room to the right edge is 72px and below the Dock is none: the app's minimum wins over both
    expect(win('calculator').frame).toMatchObject({ width: 320, height: 500 })
    wm.setFrame(id, { x: 100, y: 20, width: 5000, height: 5000 })
    expect(win('calculator').frame).toMatchObject({ width: 1280 - 100 - 8, height: 800 - 20 - 104 })
  })

  test('shrinking the workspace pulls floating windows back inside', () => {
    wm.open('notes')
    wm.setFrame(win('notes').id, { x: 900, y: 100, width: 300, height: 300 })
    wm.bounds = { width: 800, height: 600 }
    const f = win('notes').frame
    expect(f.x + f.width).toBeLessThanOrEqual(800)
    expect(f.y + f.height).toBeLessThanOrEqual(600)
  })

  test('frames of full screen and split layouts follow the workspace', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'fullscreen')
    expect(wm.frameOf(win('calculator'))).toEqual({ x: 8, y: 8, width: 1264, height: 784 })
    wm.bounds = { width: 1000, height: 700 }
    expect(wm.frameOf(win('calculator')).width).toBe(984)
  })
})

describe('layouts', () => {
  test('full screen sends the stage’s companions to the strip and hides the Dock', () => {
    wm.open('calculator')
    wm.open('notes', { alongside: true })
    wm.setLayout(win('notes').id, 'fullscreen')
    expect(wm.immersive).toBe(true)
    expect(wm.dockHidden).toBe(true)
    expect(wm.onStage.map((w) => w.appId)).toEqual(['notes'])
    expect(wm.recentStages.flatMap((s) => s.apps.map((a) => a.id))).toEqual(['calculator'])
  })

  test('split view waits for a second app, then fills the empty side', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    expect(wm.activeStage?.awaitingSplit).toBe('split-right')
    expect(wm.dockHidden).toBe(false)
    wm.open('notes')
    expect(win('notes').layout).toBe('split-right')
    expect(wm.activeStage?.awaitingSplit).toBeNull()
    expect(wm.splitDividerX).not.toBeNull()
  })

  test('the split ratio stays between a quarter and three quarters', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.bounds = { width: 2000, height: 800 }
    wm.setSplitRatio(0.05)
    expect(wm.activeStage?.splitRatio).toBe(0.25)
    wm.setSplitRatio(0.99)
    expect(wm.activeStage?.splitRatio).toBe(0.75)
  })

  test('the divider never leaves a pane narrower than its app allows', () => {
    wm.bounds = { width: 1024, height: 768 }
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.setSplitRatio(0.05)
    expect(wm.frameOf(win('calculator')).width).toBeGreaterThanOrEqual(320)
    wm.setSplitRatio(0.99)
    expect(wm.frameOf(win('notes')).width).toBeGreaterThanOrEqual(320)
  })

  test('a pane’s minimum beats the three-quarters limit when it asks for more', () => {
    setInstalledApps([
      summary('big', { window: { w: 1000, h: 600, minW: 900, minH: 300 } }),
      summary('tiny', { window: { w: 300, h: 300, minW: 100, minH: 240 } })
    ])
    wm.bounds = { width: 1100, height: 768 } // 1074 to share: 900 + 100 fits
    wm.open('big')
    wm.setLayout(win('big').id, 'split-left')
    wm.open('tiny')
    expect(wm.frameOf(win('big')).width).toBeGreaterThanOrEqual(900)
    expect(wm.frameOf(win('tiny')).width).toBeGreaterThanOrEqual(100)
    wm.setSplitRatio(0.05)
    expect(wm.frameOf(win('big')).width).toBeGreaterThanOrEqual(900)
  })

  test('a split waiting for its second app stops above the Dock; a full split fills the screen', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    expect(wm.frameOf(win('calculator')).y + wm.frameOf(win('calculator')).height).toBe(800 - 104)
    wm.open('notes')
    expect(wm.frameOf(win('calculator')).height).toBe(800 - 16)
  })

  test('the empty side of a waiting split is the pane the first window left, down to the same edge', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    const pane = wm.frameOf(win('calculator'))
    const slot = wm.splitPlaceholderFrame!
    expect(slot.y + slot.height).toBe(pane.y + pane.height)
    expect(slot.x).toBe(pane.x + pane.width + 10)
    expect(slot.x + slot.width).toBe(1280 - 8)
    wm.setSplitRatio(0.3) // the divider moves the empty side with it
    const moved = wm.splitPlaceholderFrame!
    expect(moved.x).toBe(wm.frameOf(win('calculator')).x + wm.frameOf(win('calculator')).width + 10)
    wm.open('notes')
    expect(wm.splitPlaceholderFrame).toBeNull()
  })

  test('Slide Over stops above the Dock', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'slideover')
    const f = wm.frameOf(win('calculator'))
    expect(f.y + f.height).toBe(800 - 104)
  })

  test('leaving a split returns the partner to floating', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.setLayout(win('calculator').id, 'floating')
    expect(win('notes').layout).toBe('floating')
  })

  test('only one window is in Slide Over; a second sends the first back to a stage of its own', () => {
    wm.open('calculator')
    wm.open('notes')
    wm.setLayout(win('calculator').id, 'slideover')
    wm.setLayout(win('notes').id, 'slideover')
    expect(wm.slideOver?.appId).toBe('notes')
    expect(win('calculator').layout).toBe('floating')
    expect(win('calculator').stageId).not.toBeNull()
  })

  test('minimizing Slide Over hides it; opening the app brings it back', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'slideover')
    wm.minimize(win('calculator').id)
    expect(wm.slideOverHidden).toBe(true)
    wm.open('calculator')
    expect(wm.slideOverHidden).toBe(false)
  })
})

describe('Split View sizing', () => {
  const widthOf = (appId: string): number => wm.frameOf(win(appId)).width

  test('a waiting split dragged wide is pulled back when the second app joins, so its pane meets its minimum', () => {
    wm.bounds = { width: 1024, height: 768 }
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.setSplitRatio(0.75) // only the calculator exists: its own minimum is all that can be checked
    expect(wm.activeStage?.splitRatio).toBe(0.75)
    wm.open('notes') // needs 320 too: a 75% split leaves it 249
    expect(widthOf('notes')).toBeGreaterThanOrEqual(320)
    expect(widthOf('calculator')).toBeGreaterThanOrEqual(320)
  })

  test('the ratio is clamped again when the workspace changes', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.setSplitRatio(0.75) // fine at 1280: the notes pane is 311 + ... wide
    expect(widthOf('notes')).toBeGreaterThanOrEqual(320)
    wm.bounds = { width: 1024, height: 768 }
    expect(widthOf('notes')).toBeGreaterThanOrEqual(320)
    expect(widthOf('calculator')).toBeGreaterThanOrEqual(320)
    wm.bounds = { width: 1280, height: 800 } // growing again does not move the divider
    expect(wm.activeStage?.splitRatio).toBeLessThanOrEqual(0.75)
  })

  test('where the two minimums cannot share the screen, Split View is refused and the layout is left as it was', () => {
    wm.bounds = { width: 600, height: 768 } // 574 usable: 320 + 320 does not fit
    wm.open('calculator')
    expect(wm.unavailableLayouts(win('calculator').id)).toEqual(['split-left', 'split-right']) // the menu shows them disabled
    wm.setLayout(win('calculator').id, 'split-left')
    expect(win('calculator').layout).toBe('floating')
    expect(wm.activeStage?.awaitingSplit).toBeNull()
    wm.open('notes', { alongside: true })
    wm.setLayout(win('notes').id, 'split-right') // with a partner there already
    expect(win('notes').layout).toBe('floating')
    expect(win('calculator').layout).toBe('floating')
    expect(wm.activeStage?.windowIds).toHaveLength(2)
  })

  test('an app that cannot fit beside the waiting one opens as its own stage, and the waiting pane goes back to floating', () => {
    setInstalledApps([
      summary('wide', { window: { w: 900, h: 600, minW: 700, minH: 300 } }),
      summary('roomy', { window: { w: 900, h: 600, minW: 400, minH: 300 } })
    ])
    wm.bounds = { width: 1100, height: 768 } // 1074 usable: 700 + 320 fits, 700 + 400 does not
    wm.open('wide')
    wm.setLayout(win('wide').id, 'split-left')
    expect(wm.activeStage?.awaitingSplit).toBe('split-right')
    wm.open('roomy')
    expect(win('roomy').layout).toBe('floating')
    expect(wm.notice).toContain('roomy') // the person is told why the half they waited for went away
    expect(wm.notice).toContain('wide')
    expect(wm.onStage.map((w) => w.appId)).toEqual(['roomy'])
    expect(win('wide').layout).toBe('floating')
    expect(wm.stages.every((s) => s.awaitingSplit === null)).toBe(true)
  })

  test('a screen that shrinks below both minimums dissolves the split instead of leaving two panes cut off', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.bounds = { width: 600, height: 768 }
    expect(win('calculator').layout).toBe('floating')
    expect(win('notes').layout).toBe('floating')
    for (const w of wm.windows) {
      expect(w.frame.x + w.frame.width).toBeLessThanOrEqual(600)
    }
  })

  test('a waiting split and Slide Over are never shorter than the app’s minimum, however short the screen', () => {
    wm.bounds = { width: 1280, height: 560 } // 560 - 112 = 448: the calculator needs 500
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    expect(wm.frameOf(win('calculator')).height).toBe(500)
    expect(wm.splitPlaceholderFrame!.height).toBe(500) // the empty side lines up with it
    wm.setLayout(win('calculator').id, 'slideover')
    expect(wm.frameOf(win('calculator')).height).toBe(500)
    wm.bounds = { width: 1280, height: 700 } // room enough: the Dock edge decides again
    expect(wm.frameOf(win('calculator')).height).toBe(700 - 8 - 104)
  })

  test('every layout is available on a screen with room, and a window that is gone has none disabled', () => {
    wm.open('calculator')
    expect(wm.unavailableLayouts(win('calculator').id)).toEqual([])
    expect(wm.unavailableLayouts('nope')).toEqual([])
  })

  test('Slide Over is not offered where the app’s minimum width does not fit the screen, and fits the screen otherwise', () => {
    setInstalledApps([
      summary('wide', { window: { w: 900, h: 600, minW: 480, minH: 300 } }),
      summary('notes')
    ])
    wm.bounds = { width: 360, height: 800 } // 344 to use: 480 does not fit, 320 does
    wm.open('wide')
    expect(wm.unavailableLayouts(win('wide').id)).toContain('slideover')
    wm.setLayout(win('wide').id, 'slideover')
    expect(win('wide').layout).toBe('floating')
    wm.open('notes')
    expect(wm.unavailableLayouts(win('notes').id)).not.toContain('slideover')
    wm.setLayout(win('notes').id, 'slideover')
    const f = wm.frameOf(win('notes'))
    expect(f.x).toBeGreaterThanOrEqual(8) // never off the left
    expect(f.x + f.width).toBeLessThanOrEqual(360 - 8) // nor the right
    expect(f.width).toBe(344)
  })

  test('a Slide Over whose app no longer fits when the screen shrinks goes back to a floating window on screen', () => {
    setInstalledApps([summary('wide', { window: { w: 900, h: 600, minW: 480, minH: 300 } })])
    wm.open('wide')
    wm.setLayout(win('wide').id, 'slideover')
    expect(wm.slideOver?.appId).toBe('wide')
    wm.bounds = { width: 400, height: 800 } // 384 to use: 480 no longer fits
    expect(wm.slideOver).toBeUndefined()
    expect(win('wide').layout).toBe('floating')
    expect(wm.onStage.map((w) => w.appId)).toEqual(['wide'])
    // the layout a window is in is never listed as one it cannot have (the menu would show it checked and disabled)
    expect(wm.unavailableLayouts(win('wide').id)).toEqual([
      'split-left',
      'split-right',
      'slideover'
    ])
  })

  test('a screen that shrinks under a Slide Over leaves a split beside it and the focus alone', () => {
    setInstalledApps([
      summary('calculator', { window: { w: 340, h: 560, minW: 320, minH: 500 } }),
      summary('notes'),
      summary('huge', { window: { w: 900, h: 600, minW: 700, minH: 300 } })
    ])
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'split-left')
    wm.open('notes')
    wm.open('huge', { alongside: true })
    wm.setLayout(win('huge').id, 'slideover')
    wm.focus(win('notes').id)
    wm.bounds = { width: 690, height: 800 } // 674 to use: 700 does not fit; two 320 panes (640 of 664) still do
    expect(win('huge').layout).toBe('floating')
    expect(win('calculator').layout).toBe('split-left')
    expect(win('notes').layout).toBe('split-right')
    expect(wm.focusedId).toBe(win('notes').id)
    expect(wm.activeStage?.awaitingSplit).toBeNull()
  })

  test('a Slide Over that still fits after the screen shrinks stays', () => {
    wm.open('notes')
    wm.setLayout(win('notes').id, 'slideover')
    wm.bounds = { width: 700, height: 800 }
    expect(win('notes').layout).toBe('slideover')
    expect(wm.unavailableLayouts(win('notes').id)).not.toContain('slideover')
  })

  test('the notice says where the waiting app went: the strip, or beside the new one when it opened alongside', () => {
    setInstalledApps([
      summary('wide', { window: { w: 900, h: 600, minW: 700, minH: 300 } }),
      summary('roomy', { window: { w: 900, h: 600, minW: 400, minH: 300 } })
    ])
    wm.bounds = { width: 1100, height: 768 }
    wm.open('wide')
    wm.setLayout(win('wide').id, 'split-left')
    wm.open('roomy')
    expect(wm.notice).toContain('wide is in your recent apps')
    expect(wm.recentStages.flatMap((s) => s.apps.map((a) => a.id))).toEqual(['wide'])
    wm.notice = null
    wm.close(win('roomy').id)
    wm.close(win('wide').id)
    wm.open('wide')
    wm.setLayout(win('wide').id, 'split-left')
    wm.open('roomy', { alongside: true })
    expect(wm.notice as string | null).toContain('wide is a normal window again') // (the cast: the assignment above narrowed it to null)
    expect(wm.onStage.map((w) => w.appId).sort()).toEqual(['roomy', 'wide'])
  })

  test('Slide Over is 380 wide, or the app’s minimum when that is more, and stays on screen either way', () => {
    setInstalledApps([
      summary('wide', { window: { w: 900, h: 600, minW: 480, minH: 300 } }),
      summary('notes')
    ])
    wm.open('notes')
    wm.setLayout(win('notes').id, 'slideover')
    expect(wm.frameOf(win('notes'))).toMatchObject({ width: 380, x: 1280 - 380 - 8 })
    wm.open('wide')
    wm.setLayout(win('wide').id, 'slideover')
    const f = wm.frameOf(win('wide'))
    expect(f.width).toBe(480)
    expect(f.x + f.width).toBe(1280 - 8)
  })

  test('a frame from a bigger screen is brought inside a narrow one, in every layout it passes through', () => {
    wm.open('notes')
    wm.setFrame(win('notes').id, { x: 300, y: 40, width: 800, height: 500 })
    wm.setLayout(win('notes').id, 'fullscreen') // its floating frame is kept for later
    wm.bounds = { width: 390, height: 800 }
    wm.setLayout(win('notes').id, 'floating')
    const f = wm.frameOf(win('notes'))
    expect(f.x).toBeGreaterThanOrEqual(8)
    expect(f.x + f.width).toBeLessThanOrEqual(390 - 8)
    expect(f.width).toBeGreaterThanOrEqual(320)
  })

  test('a window waiting in the strip is brought inside too, not only the ones on stage', () => {
    wm.open('notes')
    wm.setFrame(win('notes').id, { x: 300, y: 40, width: 800, height: 500 })
    wm.open('calculator') // notes goes to the strip
    wm.bounds = { width: 390, height: 800 }
    const f = win('notes').frame
    expect(f.x + f.width).toBeLessThanOrEqual(390 - 8)
    expect(f.x).toBeGreaterThanOrEqual(8)
  })

  test('leaving Slide Over for Full Screen keeps the only window on stage rather than sending it to the strip', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'slideover')
    expect(wm.activeStage).toBeUndefined()
    wm.setLayout(win('calculator').id, 'fullscreen')
    expect(win('calculator').layout).toBe('fullscreen')
    expect(wm.onStage.map((w) => w.appId)).toEqual(['calculator'])
    expect(wm.recentStages).toEqual([])
  })

  test('leaving Slide Over for a floating window or a split also lands on stage', () => {
    wm.open('calculator')
    wm.setLayout(win('calculator').id, 'slideover')
    wm.setLayout(win('calculator').id, 'floating')
    expect(wm.onStage.map((w) => w.appId)).toEqual(['calculator'])
    wm.setLayout(win('calculator').id, 'slideover')
    wm.setLayout(win('calculator').id, 'split-left')
    expect(wm.onStage.map((w) => w.appId)).toEqual(['calculator'])
    expect(wm.activeStage?.awaitingSplit).toBe('split-right')
  })
})

describe('closing, minimizing and merging', () => {
  test('closing the last window of a stage removes the stage', () => {
    wm.open('calculator')
    wm.close(win('calculator').id)
    expect(wm.windows).toEqual([])
    expect(wm.stages).toEqual([])
    expect(wm.focusedId).toBeNull()
  })

  test('closing a window focuses the topmost of what is left on stage', () => {
    wm.open('calculator')
    wm.open('notes', { alongside: true })
    wm.close(win('notes').id)
    expect(wm.focused?.appId).toBe('calculator')
  })

  test('minimize sends a window to the strip as its own group and leaves no empty stage on screen', () => {
    wm.open('calculator')
    wm.minimize(win('calculator').id)
    expect(wm.onStage).toEqual([])
    expect(wm.recentStages.flatMap((s) => s.apps.map((a) => a.id))).toEqual(['calculator'])
    expect(wm.activeStageId).toBeNull()
  })

  test('a strip group dragged onto the stage joins it, back to floating', () => {
    wm.open('calculator')
    wm.open('notes')
    const strip = wm.recentStages[0].id
    wm.mergeStage(strip)
    expect(wm.stages).toHaveLength(1)
    expect(wm.onStage.map((w) => w.appId).sort()).toEqual(['calculator', 'notes'])
  })

  test('activating a strip group puts it on stage', () => {
    wm.open('calculator')
    wm.open('notes')
    wm.activateStage(wm.recentStages[0].id)
    expect(wm.onStage.map((w) => w.appId)).toEqual(['calculator'])
  })
})

describe('an app that disappears', () => {
  test('its window leaves the strip rather than throwing while the shell catches up', () => {
    wm.open('calculator')
    wm.open('notes')
    setInstalledApps([summary('notes')]) // the calculator was uninstalled; its window was not closed yet
    expect(wm.recentStages.flatMap((s) => s.apps)).toEqual([])
    wm.setFrame(win('calculator').id, { x: 10, y: 10, width: 400, height: 400 })
  })
})
