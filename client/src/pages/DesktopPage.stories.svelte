<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import App from '../App.svelte'
  import { expect, userEvent, waitFor } from 'storybook/test'
  import { windowManager } from '../lib/windowManager.svelte'
  import { setInstalledApps } from '../lib/apps.svelte'
  import { DEFAULT_PINS, dockPins } from '../lib/dock.svelte'
  import { spotlight } from '../lib/search.svelte'
  import { isApplePlatform } from '../lib/shortcuts'
  import { FAKE_AGENTS_APP, FAKE_CALCULATOR } from '../shell/fakeHost'
  import type { AppSummary } from '../shell/hostApi'

  const closeWindows = (): void => {
    for (const win of [...windowManager.windows]) windowManager.close(win.id)
  }

  /** Every story starts as a browser that never chose: no window, the default Dock, search closed, the Calculator installed. */
  const reset = (): void => {
    closeWindows()
    spotlight.close()
    for (const id of [...dockPins.pinned]) dockPins.unpin(id)
    for (const id of DEFAULT_PINS) dockPins.pin(id)
    setInstalledApps([FAKE_CALCULATOR])
  }

  const installed = (
    id: string,
    name: string,
    icon: string,
    tint: string,
    description: string
  ): AppSummary => ({
    ...FAKE_CALCULATOR,
    id,
    name,
    icon,
    tint,
    description
  })
  /** A host with a handful of apps, listed in no particular order. */
  const SEVERAL: AppSummary[] = [
    installed('todos', 'To-dos', 'checkmark', 'green', 'Lists of things to do.'),
    FAKE_CALCULATOR,
    installed('notes', 'Notes', 'doc', 'yellow', 'Plain notes your agents can read and append to.'),
    FAKE_AGENTS_APP,
    installed('containers', 'Containers', 'cpu', 'blue', 'The containers running on this host.')
  ]
  /** More apps than any screen here has rows for. */
  const MANY: AppSummary[] = Array.from({ length: 96 }, (_, i) =>
    installed(
      `app-${String(i + 1).padStart(2, '0')}`,
      `App ${String(i + 1).padStart(2, '0')}`,
      'square.grid',
      'teal',
      'One of many.'
    )
  )

  type Canvas = {
    findByTestId: (id: string) => Promise<HTMLElement>
    getByTestId: (id: string) => HTMLElement
    queryByTestId: (id: string) => HTMLElement | null
  }

  /**
   * The Home Screen with `apps` installed: the window the desktop lands on is closed, as a person closes it, and the
   * grid of apps is what is left on the wallpaper.
   */
  const atHome = async (
    canvas: Canvas,
    apps: AppSummary[] = [FAKE_CALCULATOR]
  ): Promise<HTMLElement> => {
    await canvas.findByTestId('window-discover')
    setInstalledApps(apps)
    closeWindows()
    return canvas.findByTestId('home')
  }

  /** The apps of the Home Screen, in the order they are drawn. */
  const homeOrder = (home: HTMLElement): string[] =>
    [...home.querySelectorAll('li')].map((li) => li.dataset.app ?? '')
  /** The icons of the Dock, in order, with `|` where the divider is. */
  const dockOrder = (canvas: Canvas): string[] =>
    [...canvas.getByTestId('dock').children].map((el) =>
      el.getAttribute('data-testid') === 'dock-divider'
        ? '|'
        : (el.querySelector('button')?.getAttribute('data-testid')?.replace('dock-', '') ?? '?')
    )

  /** The modifier of the shell's shortcuts on the machine the story runs on. */
  const MOD = isApplePlatform() ? 'Meta' : 'Control'
  const searchField = async (canvas: Canvas): Promise<HTMLElement> => {
    const field = await canvas.findByTestId('spotlight-field')
    await waitFor(() => expect(field).toHaveFocus())
    return field
  }
  const resultIds = (canvas: Canvas): string[] =>
    [...canvas.getByTestId('spotlight-results').querySelectorAll('[role="option"]')].map((o) =>
      o.id.replace('spotlight-result-', '')
    )
  const searchClosed = (canvas: Canvas): Promise<void> =>
    waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())

  /** On a compact screen the app opened at launch (Discover) has the whole screen above the Dock, which stays up. */
  const presentedFullScreen = async (
    canvasElement: HTMLElement,
    canvas: Canvas,
    width: number
  ): Promise<void> => {
    const win = await canvas.findByTestId('window-discover')
    await waitFor(() => expect(win.classList.contains('fullscreen')).toBe(true))
    const screen = canvasElement.querySelector<HTMLElement>('[data-testid="desktop-screen"]')
    const left = screen?.getBoundingClientRect().left ?? 0
    const box = win.getBoundingClientRect()
    await expect(box.left - left).toBe(8)
    await expect(box.right - left).toBe(width - 8) // never past the right edge
    const dock = (await canvas.findByTestId('dock')).getBoundingClientRect()
    await expect(box.bottom).toBeLessThanOrEqual(dock.top)
  }

  /** The whole desktop shell (status bar, strip, windows, Dock) as App.svelte composes it. */
  const { Story } = defineMeta({
    title: 'Desktop/Shell',
    component: App,
    parameters: { layout: 'fullscreen' },
    beforeEach: reset,
    render: template
  })
</script>

{#snippet template(args: { width?: number; height?: number })}
  <div
    data-testid="desktop-screen"
    style:height={args.height ? `${args.height}px` : '100vh'}
    style:width={args.width ? `${args.width}px` : undefined}
  >
    <App />
  </div>
{/snippet}

<Story name="Landing" />

<Story
  name="Split View"
  play={async () => {
    windowManager.open('files')
    const files = windowManager.windows.find((w) => w.appId === 'files')!
    windowManager.setLayout(files.id, 'split-left')
    windowManager.open('settings')
  }}
/>

<!-- A phone held upright (390 px wide): one app at a time, full screen above the Dock, nothing past the right edge. -->
<Story
  name="Phone width"
  args={{ width: 390, height: 844 }}
  play={({ canvasElement, canvas }) => presentedFullScreen(canvasElement, canvas, 390)}
/>

<!-- The same phone on its side: a short screen is compact too, as on iOS. -->
<Story
  name="Phone width, on its side"
  args={{ width: 844, height: 390 }}
  play={({ canvasElement, canvas }) => presentedFullScreen(canvasElement, canvas, 844)}
/>

<!--
  The Home Screen: with no window open the wallpaper shows every app that can be opened, an icon over its name: the
  built-in pages in their fixed order, then the installed apps by name (whatever order the host listed them in). The
  "Search" capsule sits above the Dock, which holds only what is pinned (Discover, Files and Settings for someone who
  never chose).
-->
<Story
  name="Home Screen"
  play={async ({ canvas }) => {
    const home = await atHome(canvas, SEVERAL)
    await expect(home).toHaveAccessibleName('Apps')
    await expect(homeOrder(home)).toEqual([
      'discover',
      'files',
      'settings',
      'agents',
      'calculator',
      'containers',
      'notes',
      'todos'
    ])
    await expect(canvas.getByTestId('home-calculator')).toHaveAccessibleName('Calculator')
    await expect(canvas.getByTestId('home-calculator-name')).toHaveTextContent('Calculator')
    // the Dock is the pinned apps, not every app
    await expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings'])
    // the capsule is above the Dock, and the grid above the capsule
    const pill = canvas.getByTestId('home-search').getBoundingClientRect()
    await expect(pill.bottom).toBeLessThanOrEqual(
      canvas.getByTestId('dock').getBoundingClientRect().top
    )
    await expect(home.getBoundingClientRect().bottom).toBeLessThanOrEqual(pill.top)
    // nothing of the grid is under the clock widget
    const clock = canvas.getByTestId('widget-clock').getBoundingClientRect()
    for (const id of homeOrder(home)) {
      const icon = canvas.getByTestId(`home-${id}`).getBoundingClientRect()
      await expect(icon.right <= clock.left || icon.top >= clock.bottom).toBe(true)
    }
  }}
/>

<!-- A host with nothing installed still has the built-in pages. -->
<Story
  name="Home Screen, no apps installed"
  play={async ({ canvas }) => {
    const home = await atHome(canvas, [])
    await expect(homeOrder(home)).toEqual(['discover', 'files', 'settings'])
  }}
/>

<!-- More apps than the screen has rows for: the grid scrolls inside itself, above the capsule and the Dock, and every app can be reached. -->
<Story
  name="Home Screen, more apps than fit"
  play={async ({ canvas }) => {
    const home = await atHome(canvas, MANY)
    await expect(homeOrder(home)).toHaveLength(MANY.length + 3)
    await expect(home.scrollHeight).toBeGreaterThan(home.clientHeight)
    await expect(home.scrollWidth).toBeLessThanOrEqual(home.clientWidth)
    await expect(home.getBoundingClientRect().bottom).toBeLessThanOrEqual(
      canvas.getByTestId('home-search').getBoundingClientRect().top
    )
    // the keyboard reaches the last one, and brings it into view
    canvas.getByTestId('home-discover').focus()
    await userEvent.keyboard(`{${MOD}>}{End}{/${MOD}}`)
    const last = canvas.getByTestId('home-app-96')
    await expect(last).toHaveFocus()
    await waitFor(() =>
      expect(last.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        home.getBoundingClientRect().bottom
      )
    )
    await userEvent.keyboard(`{${MOD}>}{Home}{/${MOD}}`)
    await waitFor(() => expect(home.scrollTop).toBe(0))
  }}
/>

<!-- A phone (390 px wide): four columns under the clock, nothing past either edge, the capsule and the Dock below. -->
<Story
  name="Home Screen, phone width"
  args={{ width: 390, height: 844 }}
  play={async ({ canvas, canvasElement }) => {
    const home = await atHome(canvas, SEVERAL)
    const screen = canvasElement
      .querySelector<HTMLElement>('[data-testid="desktop-screen"]')!
      .getBoundingClientRect()
    const clock = canvas.getByTestId('widget-clock').getBoundingClientRect()
    const tops = new Set<number>()
    for (const id of homeOrder(home)) {
      const icon = canvas.getByTestId(`home-${id}`).getBoundingClientRect()
      tops.add(Math.round(icon.top))
      await expect(icon.left).toBeGreaterThanOrEqual(screen.left)
      await expect(icon.right).toBeLessThanOrEqual(screen.right)
      await expect(icon.top).toBeGreaterThanOrEqual(clock.bottom) // the grid starts under the widget here
    }
    await expect(tops.size).toBe(2) // eight apps, four to a row
    const dock = canvas.getByTestId('dock').getBoundingClientRect()
    await expect(dock.left).toBeGreaterThanOrEqual(screen.left)
    await expect(dock.right).toBeLessThanOrEqual(screen.right)
  }}
/>

<!--
  Opening an app from the Home Screen: a click (or Enter on the icon) opens its window, which takes the keyboard. The
  grid is gone from under the window: no icon is left to take a press or a Tab meant for it. Closing the window brings
  the Home Screen back.
-->
<Story
  name="Open from the Home Screen"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await atHome(canvas, SEVERAL)
    await userEvent.click(canvas.getByTestId('home-settings'))
    const win = await canvas.findByTestId('window-settings')
    await waitFor(() => expect(win.contains(document.activeElement)).toBe(true))
    await expect(canvas.queryByTestId('home')).toBeNull()
    await expect(canvas.queryByTestId('home-search')).toBeNull()
    closeWindows()
    // the keyboard: arrows across the grid, Enter opens
    const home = await canvas.findByTestId('home')
    canvas.getByTestId('home-discover').focus()
    await userEvent.keyboard('{ArrowRight}')
    await expect(canvas.getByTestId('home-files')).toHaveFocus()
    await userEvent.keyboard('{ArrowLeft}{ArrowRight}{ArrowRight}')
    await expect(canvas.getByTestId('home-settings')).toHaveFocus()
    await expect(home.querySelectorAll('button[tabindex="0"]')).toHaveLength(1) // one stop in the tab order
    await userEvent.keyboard('{Enter}')
    const again = await canvas.findByTestId('window-settings')
    await waitFor(() => expect(again.contains(document.activeElement)).toBe(true))
  }}
/>

<!--
  The Dock: the pinned apps, a divider, then the open apps that are not pinned, the most recent first, so the app in
  front can always be reached from the Dock. Here Settings was taken out of the Dock and is open.
-->
<Story
  name="Dock, pinned and recent apps"
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    dockPins.unpin('settings')
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'files']))
    windowManager.open('settings')
    await canvas.findByTestId('window-settings')
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'files', '|', 'settings']))
    await expect(canvas.getByTestId('dock-divider')).toHaveAccessibleName('Recent apps')
    // the recent app's icon brings its window forward, like any icon of the Dock
    windowManager.open('files')
    await canvas.findByTestId('window-files')
    await userEvent.click(canvas.getByTestId('dock-settings'))
    await waitFor(() => expect(windowManager.focused?.appId).toBe('settings'))
    await expect(dockOrder(canvas)).toEqual(['discover', 'files', '|', 'settings'])
  }}
/>

<!-- Only three recent apps are shown, the most recently used first; closing one makes room for the next. -->
<Story
  name="Dock, recent apps are capped"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    for (const id of [...dockPins.pinned]) dockPins.unpin(id)
    setInstalledApps([])
    // nothing pinned: the open app alone, with no divider
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover']))
    windowManager.open('files')
    windowManager.open('settings')
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['settings', 'files', 'discover']))
    windowManager.open('files')
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['files', 'settings', 'discover']))
    dockPins.pin('settings')
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['settings', '|', 'files', 'discover']))
    windowManager.close(windowManager.windows.find((w) => w.appId === 'files')!.id)
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['settings', '|', 'discover']))
  }}
/>

<!-- An icon's context menu, opened here on the Calculator on the Home Screen: Open, and Add to Dock (it is not pinned). -->
<Story
  name="App menu"
  play={async ({ canvas }) => {
    await atHome(canvas, SEVERAL)
    const icon = canvas.getByTestId('home-calculator')
    await expect(icon).toHaveAttribute('aria-haspopup', 'menu')
    await userEvent.pointer({ keys: '[MouseRight]', target: icon })
    const menu = await canvas.findByTestId('app-menu')
    await expect(menu).toHaveAccessibleName('Calculator')
    await expect(
      [...menu.querySelectorAll('[role="menuitem"]')].map((i) => i.textContent?.trim())
    ).toEqual(['Open', 'Add to Dock'])
    await waitFor(() => expect(canvas.getByTestId('app-menu-open')).toHaveFocus())
    // beside the icon, on the screen
    const box = menu.getBoundingClientRect()
    await expect(box.top).toBeGreaterThanOrEqual(icon.getBoundingClientRect().bottom)
  }}
/>

<!--
  Pinning and unpinning with the pointer: a secondary click on an icon of the Home Screen offers Add to Dock; the app
  joins the end of the Dock. The same on its icon in the Dock offers Remove from Dock, and the menu opens above it. A
  click outside the menu closes it and presses nothing. Open opens the app.
-->
<Story
  name="Pin and unpin, by pointer"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await atHome(canvas, SEVERAL)
    const right = (id: string): Promise<unknown> =>
      userEvent.pointer({ keys: '[MouseRight]', target: canvas.getByTestId(id) })
    const menuGone = (): Promise<void> =>
      waitFor(() => expect(canvas.queryByTestId('app-menu')).toBeNull())

    await right('home-calculator')
    await userEvent.click(await canvas.findByTestId('app-menu-pin'))
    await menuGone()
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings', 'calculator'])
    )
    await expect(dockPins.pinned).toEqual(['discover', 'files', 'settings', 'calculator'])

    // a second one goes after it: the order they were added in
    await right('home-agents')
    await userEvent.click(await canvas.findByTestId('app-menu-pin'))
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings', 'calculator', 'agents'])
    )

    // on the Home Screen a pinned app now offers the way back
    await right('home-calculator')
    await expect(await canvas.findByTestId('app-menu-unpin')).toHaveTextContent('Remove from Dock')
    await expect(canvas.queryByTestId('app-menu-pin')).toBeNull()
    // a press outside closes the menu, and does not open the app under the pointer
    await userEvent.pointer({ keys: '[MouseLeft]', target: canvas.getByTestId('app-menu-layer') })
    await menuGone()
    await expect(windowManager.windows).toHaveLength(0)
    await expect(dockOrder(canvas)).toContain('calculator')

    // from the Dock: the menu opens above the icon
    await right('dock-calculator')
    const menu = await canvas.findByTestId('app-menu')
    await waitFor(() =>
      expect(menu.getBoundingClientRect().bottom).toBeLessThanOrEqual(
        canvas.getByTestId('dock-calculator').getBoundingClientRect().top
      )
    )
    await userEvent.click(canvas.getByTestId('app-menu-unpin'))
    await menuGone()
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings', 'agents'])
    )

    // a built-in page can be taken out too, and Open opens
    await right('dock-files')
    await userEvent.click(await canvas.findByTestId('app-menu-unpin'))
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'settings', 'agents']))
    await right('home-files')
    await userEvent.click(await canvas.findByTestId('app-menu-open'))
    const win = await canvas.findByTestId('window-files')
    await waitFor(() => expect(win.contains(document.activeElement)).toBe(true))
    // open and not pinned: it is in the Dock's recent apps
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'settings', 'agents', '|', 'files'])
    )
  }}
/>

<!--
  The same with the keyboard: Shift+F10 (or the Menu key) on an icon opens its menu with focus on the first item; the
  arrows move, Enter chooses, Escape closes; focus comes back to the icon the menu was opened on. When unpinning takes
  the Dock's icon away, focus goes to the app's icon on the Home Screen.
-->
<Story
  name="Pin and unpin, by keyboard"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await atHome(canvas, SEVERAL)
    const menuGone = (): Promise<void> =>
      waitFor(() => expect(canvas.queryByTestId('app-menu')).toBeNull())
    const icon = canvas.getByTestId('home-calculator')
    icon.focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await canvas.findByTestId('app-menu')
    await waitFor(() => expect(canvas.getByTestId('app-menu-open')).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}')
    await expect(canvas.getByTestId('app-menu-pin')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}') // the last item: stays
    await expect(canvas.getByTestId('app-menu-pin')).toHaveFocus()
    await userEvent.keyboard('{Escape}')
    await menuGone()
    await waitFor(() => expect(icon).toHaveFocus())
    await expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings'])

    // the Menu key; Enter on Add to Dock
    await userEvent.keyboard('{ContextMenu}')
    await canvas.findByTestId('app-menu')
    await waitFor(() => expect(canvas.getByTestId('app-menu-open')).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}{Enter}')
    await menuGone()
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings', 'calculator'])
    )
    await waitFor(() => expect(icon).toHaveFocus())

    // on the Dock's icon: Escape gives focus back to it
    const docked = canvas.getByTestId('dock-calculator')
    docked.focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await canvas.findByTestId('app-menu')
    await waitFor(() => expect(canvas.getByTestId('app-menu-open')).toHaveFocus())
    await userEvent.keyboard('{Escape}')
    await menuGone()
    await waitFor(() => expect(docked).toHaveFocus())

    // Remove from Dock: the icon the menu was opened on is gone, so focus goes to the app on the Home Screen
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await canvas.findByTestId('app-menu')
    await waitFor(() => expect(canvas.getByTestId('app-menu-open')).toHaveFocus())
    await userEvent.keyboard('{End}')
    await expect(canvas.getByTestId('app-menu-unpin')).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await menuGone()
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'files', 'settings']))
    await waitFor(() => expect(canvas.getByTestId('home-calculator')).toHaveFocus())
  }}
/>

<!-- An app that is not there (not installed yet, uninstalled) keeps its pin: it is skipped in the Dock, and back in its place when the app is. -->
<Story
  name="A pinned app that is not installed"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await atHome(canvas, SEVERAL)
    for (const id of [...dockPins.pinned]) dockPins.unpin(id)
    for (const id of ['discover', 'calculator', 'files']) dockPins.pin(id)
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'calculator', 'files']))
    setInstalledApps([]) // the host has nothing running (or has not answered yet)
    await waitFor(() => expect(dockOrder(canvas)).toEqual(['discover', 'files']))
    await expect(dockPins.pinned).toEqual(['discover', 'calculator', 'files']) // still the person's
    dockPins.pin('settings')
    setInstalledApps(SEVERAL)
    await waitFor(() =>
      expect(dockOrder(canvas)).toEqual(['discover', 'calculator', 'files', 'settings'])
    )
  }}
/>

<!--
  System search, opened from the status bar's magnifier (always there, whatever covers the screen): over the dimmed,
  blurred desktop, the field focused, and before anything is typed the apps opened most recently.
-->
<Story
  name="Spotlight, suggestions"
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    setInstalledApps(SEVERAL)
    windowManager.open('files')
    await canvas.findByTestId('window-files')
    const button = canvas.getByTestId('status-bar-search')
    await expect(button).toHaveAccessibleName('Search')
    await userEvent.click(button)
    const field = await searchField(canvas)
    await expect(field).toHaveValue('')
    await expect(canvas.getByTestId('spotlight')).toHaveAttribute('aria-modal', 'true')
    // most recent first (Files, then the Discover the desktop landed on), then the first of the Home Screen
    await expect(resultIds(canvas)).toEqual([
      'app-files',
      'app-discover',
      'app-settings',
      'app-agents'
    ])
    await expect(canvas.getByTestId('spotlight-results')).toHaveAccessibleName('Suggestions')
    await expect(field).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-files')
    await waitFor(() =>
      expect(canvas.getByTestId('spotlight-status')).toHaveTextContent('4 suggestions')
    )
  }}
/>

<!--
  A query: apps and built-in pages by name, Settings panes by name, an installed app by what it says it is for (below
  the names). The start of a name comes first, then a match inside one.
-->
<Story
  name="Spotlight, results"
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    setInstalledApps(SEVERAL)
    await userEvent.click(canvas.getByTestId('status-bar-search'))
    const field = await searchField(canvas)
    await userEvent.type(field, 'app')
    // "Appearance" (a Settings pane, by name), then Notes: its description says "append"
    await waitFor(() => expect(resultIds(canvas)).toEqual(['settings-appearance', 'app-notes']))
    await expect(canvas.getByTestId('spotlight-result-settings-appearance')).toHaveTextContent(
      'Settings'
    )
    await expect(canvas.getByTestId('spotlight-result-app-notes')).toHaveTextContent('App')
    await expect(canvas.getByTestId('spotlight-status')).toHaveTextContent('2 results')

    await userEvent.clear(field)
    await userEvent.type(field, 'G')
    // the start of a name first (General), then inside one: the built-in page before the installed app
    await waitFor(() =>
      expect(resultIds(canvas).slice(0, 3)).toEqual([
        'settings-general',
        'app-settings',
        'app-agents'
      ])
    )
    await expect(field).toHaveAttribute(
      'aria-activedescendant',
      'spotlight-result-settings-general'
    )
    // what follows matched on what an app says it is for, below every name
    await expect(resultIds(canvas).length).toBeGreaterThan(3)
    await expect(canvas.getByTestId('spotlight-status')).toHaveTextContent(
      `${resultIds(canvas).length} results`
    )
  }}
/>

<!-- Nothing found: said in the panel and to a screen reader; Enter opens nothing. Diacritics and case never matter. -->
<Story
  name="Spotlight, no results"
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    await userEvent.click(canvas.getByTestId('status-bar-search'))
    const field = await searchField(canvas)
    await userEvent.type(field, 'CÁLC')
    await waitFor(() => expect(resultIds(canvas)).toEqual(['app-calculator']))
    await userEvent.clear(field)
    await userEvent.type(field, 'zebra')
    await expect(await canvas.findByTestId('spotlight-empty')).toHaveTextContent('No Results')
    await expect(canvas.getByTestId('spotlight-status')).toHaveTextContent('No results')
    await userEvent.keyboard('{Enter}')
    await expect(canvas.getByTestId('spotlight')).toBeInTheDocument()
    await expect(windowManager.windows.map((w) => w.appId)).toEqual(['discover'])
  }}
/>

<!--
  The keyboard, from the shortcut to the window: Cmd+K (Ctrl+K off a Mac) opens search and pressed again closes it;
  typing filters; Down and Up move the highlight; Enter opens it. A Settings pane opens Settings on that pane, with the
  keyboard in its window; opening another pane while Settings is open changes the pane.
-->
<Story
  name="Spotlight, keyboard"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    setInstalledApps(SEVERAL)
    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    await searchField(canvas)
    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    await searchClosed(canvas)

    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    const field = await searchField(canvas)
    await userEvent.keyboard('a')
    await waitFor(() =>
      expect(resultIds(canvas).slice(0, 2)).toEqual(['app-agents', 'settings-appearance'])
    )
    await userEvent.keyboard('{ArrowDown}')
    await expect(field).toHaveAttribute(
      'aria-activedescendant',
      'spotlight-result-settings-appearance'
    )
    await expect(canvas.getByTestId('spotlight-result-settings-appearance')).toHaveAttribute(
      'aria-selected',
      'true'
    )
    await userEvent.keyboard('{ArrowUp}{ArrowUp}{ArrowDown}')
    await expect(field).toHaveAttribute(
      'aria-activedescendant',
      'spotlight-result-settings-appearance'
    )
    await expect(field).toHaveFocus()
    await userEvent.keyboard('{Enter}')
    await searchClosed(canvas)
    const settings = await canvas.findByTestId('window-settings')
    await expect(await canvas.findByTestId('settings-theme')).toBeVisible() // the Appearance pane
    await waitFor(() => expect(settings.contains(document.activeElement)).toBe(true))
    await expect(windowManager.focused?.appId).toBe('settings')

    // Settings is open already: another pane, by its top result
    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    await searchField(canvas)
    await userEvent.keyboard('devi{Enter}')
    await searchClosed(canvas)
    await expect(await canvas.findByTestId('settings-devices')).toBeVisible()
    await waitFor(() => expect(settings.contains(document.activeElement)).toBe(true))

    // an app by name; and "Show Home Screen", which leaves the stage without closing anything
    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    await searchField(canvas)
    await userEvent.keyboard('fil{Enter}')
    const files = await canvas.findByTestId('window-files')
    await waitFor(() => expect(files.contains(document.activeElement)).toBe(true))
    await userEvent.keyboard(`{${MOD}>}k{/${MOD}}`)
    await searchField(canvas)
    await userEvent.keyboard('home')
    await waitFor(() => expect(resultIds(canvas)).toEqual(['action-home']))
    await userEvent.keyboard('{Enter}')
    await searchClosed(canvas)
    const home = await canvas.findByTestId('home')
    await expect(windowManager.windows.map((w) => w.appId).sort()).toEqual([
      'discover',
      'files',
      'settings'
    ])
    await waitFor(() => expect(home.contains(document.activeElement)).toBe(true))
  }}
/>

<!--
  Escape: the first clears what was typed, the second closes search, and focus is back where it was: the "Search"
  capsule of the Home Screen here, the status bar's button when it was opened from there. A click on the dimmed desktop
  closes it too. While it is open the keys are its own: the window behind hears none.
-->
<Story
  name="Spotlight, Escape twice"
  tags={['play-only']}
  play={async ({ canvas, canvasElement }) => {
    await atHome(canvas, SEVERAL)
    const pill = canvas.getByTestId('home-search')
    await expect(pill).toHaveAccessibleName('Search')
    pill.focus()
    await userEvent.keyboard('{Enter}')
    const field = await searchField(canvas)
    await userEvent.keyboard('calc')
    await expect(field).toHaveValue('calc')
    await userEvent.keyboard('{Escape}')
    await expect(field).toHaveValue('')
    await expect(canvas.getByTestId('spotlight')).toBeInTheDocument()
    await expect(canvas.getByTestId('spotlight-results')).toHaveAccessibleName('Suggestions')
    await userEvent.keyboard('{Escape}')
    await searchClosed(canvas)
    await waitFor(() => expect(pill).toHaveFocus())
    await expect(windowManager.windows).toHaveLength(0) // nothing was opened

    // from the status bar, over a window: Escape gives the button its focus back, and the window heard no key
    windowManager.open('files')
    await canvas.findByTestId('window-files')
    const heard: string[] = []
    const listen = (e: KeyboardEvent): void => void heard.push(e.key)
    const view = canvasElement.ownerDocument.defaultView!
    view.addEventListener('keydown', listen)
    try {
      const button = canvas.getByTestId('status-bar-search')
      button.focus()
      await userEvent.keyboard('{Enter}')
      heard.length = 0
      await searchField(canvas)
      await userEvent.keyboard('x{ArrowDown}{Escape}{Escape}')
      await searchClosed(canvas)
      await waitFor(() => expect(button).toHaveFocus())
      await expect(heard).toEqual([])

      // a click outside the panel
      await userEvent.click(button)
      await searchField(canvas)
      await userEvent.pointer({
        keys: '[MouseLeft]',
        target: canvas.getByTestId('spotlight-scrim')
      })
      await searchClosed(canvas)
      await waitFor(() => expect(button).toHaveFocus())
    } finally {
      view.removeEventListener('keydown', listen)
    }
  }}
/>

<!-- A Split View that waits for its second app shows the apps in its empty half: that is where the app is chosen. -->
<Story
  name="Split View, choosing the second app"
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    setInstalledApps(SEVERAL)
    windowManager.open('files')
    windowManager.setLayout(
      windowManager.windows.find((w) => w.appId === 'files')!.id,
      'split-left'
    )
    const placeholder = await canvas.findByTestId('split-placeholder')
    await expect(placeholder).toHaveTextContent('Choose an app')
    const home = await canvas.findByTestId('home')
    await expect(placeholder.contains(home)).toBe(true)
    await expect(canvas.queryByTestId('home-search')).toBeNull()
    const pane = placeholder.getBoundingClientRect()
    for (const id of homeOrder(home)) {
      const icon = canvas.getByTestId(`home-${id}`).getBoundingClientRect()
      await expect(icon.left).toBeGreaterThanOrEqual(pane.left)
      await expect(icon.right).toBeLessThanOrEqual(pane.right)
    }
  }}
/>

<!-- Choosing from the waiting half fills it: the app opens beside the first, with the keyboard. -->
<Story
  name="Split View, the second app chosen"
  tags={['play-only']}
  play={async ({ canvas }) => {
    await canvas.findByTestId('window-discover')
    windowManager.open('files')
    windowManager.setLayout(
      windowManager.windows.find((w) => w.appId === 'files')!.id,
      'split-left'
    )
    await canvas.findByTestId('split-placeholder')
    await userEvent.click(await canvas.findByTestId('home-settings'))
    const settings = await canvas.findByTestId('window-settings')
    await waitFor(() => expect(settings.classList.contains('split-right')).toBe(true))
    await expect(canvas.queryByTestId('split-placeholder')).toBeNull()
    await expect(canvas.queryByTestId('home')).toBeNull()
    await waitFor(() => expect(settings.contains(document.activeElement)).toBe(true))
  }}
/>
