<!--
  @page DesktopPage — the vos desktop, iPadOS Stage Manager style: status
  bar, recent-apps strip, the current stage (floating / full screen / split
  windows), Slide Over, widgets and the dock. On a compact screen (a phone) every window is shown full screen above
  the Dock, one app at a time (`WindowManager.compact`). Window contents are supplied by
  the composition root through the `windowContent` snippet.

  The Home Screen: while no window is on stage (`WindowManager.atHome`) the wallpaper shows every app that can be
  opened (the built-in pages, then the running installed apps by name: `appOrder`) as a grid of icons, with a
  "Search" capsule above the Dock. It is not drawn at all under a window, so nothing of it takes a press or a key meant
  for the window. While a Split View waits for its second app, the same grid fills the empty half: that is where the
  app is chosen. Opening an app from the grid sends the keyboard to its window (the icon that had it is gone).

  The Dock holds the apps the person pinned (`dockPins`), then, after a divider, up to three open apps that are not
  pinned, most recent first (`recentApps`). An icon of the Dock or the Home Screen has a context menu (right-click, a
  long press, the Menu key or Shift+F10): Open, and Add to Dock or Remove from Dock. Leaving the menu, or pinning,
  gives focus back to the icon it was opened on (or the app's other icon when that one went with the change).

  System search (`spotlight`): the magnifier in the status bar, the "Search" capsule, and Cmd+K (Ctrl+K off a Mac)
  from anywhere in the shell open it; opening a result sends the keyboard to that app's window. A key pressed inside
  an app's frame never reaches this page (the frame is sandboxed and its bridge sends no keys), so the shortcut does
  not work while an app has the keyboard: the status bar's button does.

  Test ids: `status-bar` (+ `status-bar-search`), `desktop-stage`, `widget-clock`, `home-screen`, `home`
  (+ `home-${appId}`, see HomeGrid), `home-search`,
  `stage-strip` (+ `stage-strip-<appIds>`), `window-${appId}` (+ parts, see Window),
  `split-divider`, `split-placeholder`, `slideover-tab`, `dock` (+ `dock-${appId}`, `dock-divider`), `app-menu`
  (+ `-open`, `-pin`, `-unpin`, `-layer`), `spotlight` (+ parts, see Spotlight), `desktop-notice` (+ `-dismiss`), and
  the system alerts (see NotificationBanners).
-->
<script lang="ts">
  import { tick, untrack, type Snippet } from 'svelte'
  import type { AnchorRect, DockApp, MenuItem } from '@vos/ui'
  import { SplitHandle, Toast } from '@vos/ui/atoms'
  import { ContextMenu, SearchPill, Spotlight } from '@vos/ui/molecules'
  import { DesktopTemplate } from '@vos/ui/templates'
  import { StatusBar, Dock, HomeGrid, Window, StageStrip } from '@vos/ui/organisms'
  import ClockWidget from '../organisms/ClockWidget.svelte'
  import NotificationBanners from '../organisms/NotificationBanners.svelte'
  import { appOf, appOrder, installedApp, onAppsRemoved, type AppId } from '../lib/apps.svelte'
  import { dockPins, recentApps } from '../lib/dock.svelte'
  import { recentlyOpened, spotlight } from '../lib/search.svelte'
  import { isApplePlatform, isSearchShortcut, searchShortcut } from '../lib/shortcuts'
  import { windowManager as wm, type WindowState } from '../lib/windowManager.svelte'
  import { host } from '../lib/host.svelte'
  import { listenToHost, notifications } from '../lib/notifications.svelte'
  import { settings } from '../lib/settings.svelte'

  interface Props {
    windowContent: Snippet<[WindowState]>
  }

  let { windowContent }: Props = $props()

  // What an app's backend tells the person (`notification.deliver`) is drawn here, above every window; an app that
  // stops running takes its alerts with it.
  let bannersHeight = $state(0)
  $effect(() => listenToHost(host))
  $effect(() => onAppsRemoved((ids) => notifications.forgetApps(ids)))
  // banners turned off in Settings (here, or in another tab of this browser): the ones on screen go too
  // (the setting is all this follows: what is on screen is read without being watched, or clearing it would run this again)
  $effect(() => {
    if (!settings.banners) untrack(() => notifications.clear())
  })

  /** Where the keys go when a banner that had them leaves and nothing nearer is left: the front window, else the Dock. */
  function restingFocus(): HTMLElement | null {
    const front = wm.focused
      ? document.querySelector<HTMLElement>(`[data-testid="window-${wm.focused.appId}"]`)
      : null
    return front ?? document.querySelector<HTMLElement>('[data-testid="dock"] button')
  }

  let workspaceWidth = $state(0)
  let workspaceHeight = $state(0)

  $effect(() => {
    if (workspaceWidth && workspaceHeight)
      wm.bounds = { width: workspaceWidth, height: workspaceHeight }
  })

  // The link to the host: say so in the status bar while it's down.
  const connection = $derived(host.connection)

  /** An app as an icon: `running` (the dot under it) is the Dock's to show, not the Home Screen's. */
  function toIcon(id: AppId, inDock: boolean): DockApp[] {
    const app = appOf(id)
    return app
      ? [
          {
            id,
            name: app.name,
            icon: app.icon,
            background: app.background,
            running: inDock && wm.isOpen(id)
          }
        ]
      : []
  }

  // The Dock: what the person pinned (an id whose app is not there now is skipped, and kept), then the open apps
  // that are not pinned.
  const pinnedApps = $derived(
    dockPins.shown((id) => appOf(id) !== undefined).flatMap((id) => toIcon(id, true))
  )
  const recentDockApps = $derived(
    recentApps(wm.windows, (id) => dockPins.isPinned(id)).flatMap((id) => toIcon(id, true))
  )
  // The Home Screen: every app, in the one order.
  const homeApps = $derived(appOrder().flatMap((id) => toIcon(id, false)))
  const awaitingSplit = $derived(wm.activeStage?.awaitingSplit ?? null)

  const byTestId = (testid: string): HTMLElement | null =>
    document.querySelector<HTMLElement>(`[data-testid="${CSS.escape(testid)}"]`)

  /**
   * Send the keyboard to an app's window: to its frame when the app's page is up, else to the window itself (a
   * built-in page; an app still opening, whose frame takes over from its window when it is ready: `focusFrame`).
   */
  async function focusWindow(appId: AppId): Promise<void> {
    await tick()
    const win = byTestId(`window-${appId}`)
    if (!win) return
    const frame = win.querySelector<HTMLIFrameElement>('iframe:not(.hidden)')
    if (frame) frame.focus({ preventScroll: true })
    else win.focus({ preventScroll: true })
  }

  /** Open an app from the Home Screen: its icon goes away with the grid, so the keys go to the window. */
  function openFromHome(id: string): void {
    wm.open(id)
    void focusWindow(id)
  }

  // ── The context menu of an app's icon ──
  let menu = $state<{ appId: AppId; anchor: HTMLElement; rect: AnchorRect } | null>(null)
  const menuApp = $derived(menu ? appOf(menu.appId) : undefined)
  const menuItems = $derived<MenuItem[]>(
    menu
      ? [
          { id: 'open', label: 'Open', icon: 'arrow.up.forward.square' },
          dockPins.isPinned(menu.appId)
            ? { id: 'unpin', label: 'Remove from Dock', icon: 'dock.rectangle.slash' }
            : { id: 'pin', label: 'Add to Dock', icon: 'dock.rectangle' }
        ]
      : []
  )

  function askMenu(id: string, anchor: HTMLElement): void {
    const { left, top, width, height } = anchor.getBoundingClientRect()
    menu = { appId: id, anchor, rect: { left, top, width, height } }
  }

  /**
   * Focus back on the icon the menu was opened on. Pinning can take that icon away (an app that is not open leaves the
   * Dock when it is unpinned; one that is open moves across the divider): then the app's icon wherever it is now, the
   * Home Screen's first, else where the keys rest.
   */
  async function focusIcon(anchor: HTMLElement, appId: AppId): Promise<void> {
    await tick()
    const icon = anchor.isConnected
      ? anchor
      : (byTestId(`home-${appId}`) ?? byTestId(`dock-${appId}`) ?? restingFocus())
    icon?.focus({ preventScroll: true })
  }

  function dismissMenu(): void {
    if (!menu) return
    const { anchor, appId } = menu
    menu = null
    void focusIcon(anchor, appId)
  }

  function chooseFromMenu(item: string): void {
    if (!menu) return
    const { anchor, appId } = menu
    menu = null
    if (item === 'open') {
      wm.open(appId)
      void focusWindow(appId)
      return
    }
    if (item === 'pin') dockPins.pin(appId)
    else dockPins.unpin(appId)
    void focusIcon(anchor, appId)
  }

  // an app that stops running takes its menu with it
  $effect(() => {
    if (menu && !menuApp) menu = null
  })

  // ── System search ──
  const mac = isApplePlatform()
  // what was brought to the front, most recent first: what search suggests before anything is typed
  $effect(() => {
    const front = wm.focused?.appId
    if (front) untrack(() => recentlyOpened.note(front))
  })

  function openSearch(): void {
    menu = null
    spotlight.show()
  }

  /** Open a result: the keyboard follows it to the window it opened, or to the Home Screen it showed. */
  function openResult(id: string): void {
    const result = spotlight.activate(id)
    if (!result) return
    if (result.opens) void focusWindow(result.opens)
    else
      void tick().then(() =>
        document
          .querySelector<HTMLElement>('[data-testid="home"] button[tabindex="0"]')
          ?.focus({ preventScroll: true })
      )
  }

  /** Cmd+K / Ctrl+K, heard before anything else on the page: it opens search, and closes it again. */
  function onshortcut(e: KeyboardEvent): void {
    if (!isSearchShortcut(e, mac)) return
    e.preventDefault()
    e.stopPropagation()
    menu = null
    spotlight.toggle()
  }

  // Dragging a strip group onto the stage merges it into the current group.
  function ondragover(e: DragEvent): void {
    if (e.dataTransfer?.types.includes('application/x-vos-stage')) e.preventDefault()
  }
  function ondrop(e: DragEvent): void {
    const id = e.dataTransfer?.getData('application/x-vos-stage')
    if (id) {
      e.preventDefault()
      wm.mergeStage(id)
    }
  }

  let dividerStart = 0

  // A layout the manager changed on its own is explained in a toast that goes after a while, and waits while it is read.
  const NOTICE_MS = 9000
  let noticeHeld = $state(false)
  $effect(() => {
    if (!wm.notice) {
      noticeHeld = false // the toast went (dismissed under the pointer): its `onrelease` never comes, so let go here
      return undefined
    }
    if (noticeHeld) return undefined
    const timer = setTimeout(() => (wm.notice = null), NOTICE_MS)
    return () => clearTimeout(timer)
  })

  // Widgets sit on the wallpaper; like iPadOS, they step back (fade out) as soon
  // as a window covers any part of them, instead of peeking out from behind it.
  let widgetsWidth = $state(0)
  let widgetsHeight = $state(0)
  // (The empty half of a waiting Split View, where the apps are listed to choose from, covers them like a window.)
  const widgetsObscured = $derived(
    [
      ...wm.onStage.map((w) => wm.frameOf(w)),
      ...(wm.slideOver && !wm.slideOverHidden ? [wm.frameOf(wm.slideOver)] : []),
      ...(wm.splitPlaceholderFrame ? [wm.splitPlaceholderFrame] : [])
    ].some((f) => f.x + f.width > wm.bounds.width - 24 - widgetsWidth && f.y < 16 + widgetsHeight)
  )

  // The Home Screen's grid is centred between the strip's width at each side (the screen's edge on a compact screen,
  // where there is no strip). Where it would run under the widgets it starts below them instead.
  const HOME_GRID_MAX = 736 // HomeGrid's eight columns
  const homeSide = $derived(wm.compact ? 16 : 150)
  const homeTop = $derived.by(() => {
    const width = wm.bounds.width
    const gridRight = (width + Math.min(HOME_GRID_MAX, width - homeSide * 2)) / 2
    const clearOfWidgets = widgetsWidth === 0 || gridRight <= width - 24 - widgetsWidth - 16
    return clearOfWidgets ? 24 : 16 + widgetsHeight + 16
  })
</script>

{#snippet homeGrid()}
  <HomeGrid testid="home" apps={homeApps} onselect={openFromHome} onmenu={askMenu} />
{/snippet}

<DesktopTemplate
  bind:workspaceWidth
  bind:workspaceHeight
  dockHidden={wm.dockHidden}
  statusBarHidden={!settings.statusBar}
>
  {#snippet statusBar()}
    <div data-testid="status-bar">
      <StatusBar
        testid="status-bar"
        bare={!settings.statusBar}
        onsearch={openSearch}
        searchShortcut={searchShortcut(mac)}
      >
        {#snippet center()}
          {#if connection !== 'online'}
            <span class="connection" role="status" data-testid="status-connection">
              {connection === 'connecting'
                ? 'Connecting to vos…'
                : connection === 'incompatible'
                  ? 'Reload to update vos'
                  : 'Reconnecting to vos…'}
            </span>
          {/if}
        {/snippet}
      </StatusBar>
    </div>
  {/snippet}

  {#snippet workspace()}
    <div class="stage" role="presentation" data-testid="desktop-stage" {ondragover} {ondrop}>
      <!-- first in the page, so Tab reaches a banner before anything else on the desktop -->
      <NotificationBanners
        center={notifications}
        bind:height={bannersHeight}
        fallback={restingFocus}
      />

      {#if wm.atHome}
        <div
          class="home"
          data-testid="home-screen"
          style:padding-top="{homeTop}px"
          style:padding-inline="{homeSide}px"
        >
          <div class="home-apps">{@render homeGrid()}</div>
          <SearchPill testid="home-search" shortcut={searchShortcut(mac)} onclick={openSearch} />
        </div>
      {/if}

      <div
        class="widgets"
        class:obscured={widgetsObscured}
        aria-hidden={widgetsObscured}
        inert={widgetsObscured}
        bind:clientWidth={widgetsWidth}
        bind:clientHeight={widgetsHeight}
      >
        <ClockWidget testid="widget-clock" />
      </div>

      <StageStrip
        testid="stage-strip"
        stages={wm.recentStages}
        autoHide={wm.stripObscured}
        onselect={(id) => wm.activateStage(id)}
      />

      {#each wm.windows as win (win.id)}
        {@const app = appOf(win.appId)}
        {@const onStage = win.stageId === wm.activeStageId || win.layout === 'slideover'}
        <!-- An installed app keeps its page for as long as its window exists: off the active stage the window is
             kept in the page, hidden (`display: none` keeps the frame's document: it is not reloaded when shown again, takes
             no focus or pointer events and is not read by a screen reader), so what the app holds in its page survives
             switching apps. The built-in pages are drawn again when their stage returns. -->
        {#if app && (onStage || installedApp(win.appId))}
          {@const frame = wm.frameOf(win)}
          <div class="slot" class:away={!onStage} inert={!onStage} aria-hidden={!onStage}>
            <Window
              testid="window-{win.appId}"
              title={app.name}
              x={frame.x}
              y={frame.y}
              width={frame.width}
              height={frame.height}
              minWidth={app.minWidth}
              minHeight={app.minHeight}
              z={wm.layoutOf(win) === 'slideover' ? 10000 : win.z}
              layout={wm.layoutOf(win)}
              unavailable={wm.unavailableLayouts(win.id)}
              focused={wm.focusedId === win.id}
              onframechange={(f) => wm.setFrame(win.id, f)}
              onfocus={() => wm.focus(win.id)}
              onlayout={(l) => wm.setLayout(win.id, l)}
              onminimize={() => wm.minimize(win.id)}
              onclose={() => wm.close(win.id)}
            >
              {@render windowContent(win)}
            </Window>
          </div>
        {/if}
      {/each}

      {#if wm.splitDividerX !== null}
        <div class="divider" style:left="{wm.splitDividerX}px">
          <SplitHandle
            testid="split-divider"
            ondrag={(dx) => wm.moveSplitDivider(dividerStart + dx)}
            ondragend={() => (dividerStart = wm.splitDividerX ?? 0)}
          />
        </div>
      {/if}

      {#if awaitingSplit && wm.splitPlaceholderFrame}
        {@const pane = wm.splitPlaceholderFrame}
        <div
          class="split-placeholder"
          data-testid="split-placeholder"
          style:left="{pane.x}px"
          style:top="{pane.y}px"
          style:width="{pane.width}px"
          style:height="{pane.height}px"
        >
          <p>Choose an app</p>
          <div class="home-apps">{@render homeGrid()}</div>
        </div>
      {/if}

      <!-- The live region is always mounted and only its text changes: a region inserted with its text already in it
           is often not announced. The toast is what is seen; it is not a live region itself. -->
      <div class="sr-only" role="status" aria-live="polite" data-testid="desktop-notice-live">
        {wm.notice ?? ''}
      </div>
      {#if wm.notice}
        <div class="notice" style:top="{8 + (bannersHeight ? bannersHeight + 8 : 0)}px">
          <Toast
            testid="desktop-notice"
            live={false}
            text={wm.notice}
            ondismiss={() => (wm.notice = null)}
            onhold={() => (noticeHeld = true)}
            onrelease={() => (noticeHeld = false)}
          />
        </div>
      {/if}

      {#if wm.slideOver && wm.slideOverHidden}
        <button
          type="button"
          class="slideover-tab"
          data-testid="slideover-tab"
          aria-label="Show Slide Over"
          onclick={() => wm.toggleSlideOver()}
        ></button>
      {/if}
    </div>
  {/snippet}

  {#snippet dock()}
    <Dock
      available={workspaceWidth > 0 ? workspaceWidth - 24 : undefined}
      testid="dock"
      apps={pinnedApps}
      recents={recentDockApps}
      onselect={(id) => wm.open(id)}
      onmenu={askMenu}
    />
  {/snippet}

  {#snippet overlay()}
    {#if menu && menuApp}
      <ContextMenu
        testid="app-menu"
        label={menuApp.name}
        items={menuItems}
        anchor={menu.rect}
        onselect={chooseFromMenu}
        ondismiss={dismissMenu}
      />
    {/if}
    {#if spotlight.open}
      <Spotlight
        testid="spotlight"
        bind:query={spotlight.query}
        items={spotlight.results}
        activeId={spotlight.activeId}
        heading={spotlight.suggesting ? 'Suggestions' : undefined}
        status={spotlight.status}
        onmove={(delta) => spotlight.move(delta)}
        onhighlight={(id) => spotlight.highlight(id)}
        onactivate={openResult}
        ondismiss={() => spotlight.close()}
      />
    {/if}
  {/snippet}
</DesktopTemplate>

<svelte:window
  onkeydowncapture={onshortcut}
  onpointerdown={() => {
    // Remember the divider origin for the next drag.
    dividerStart = wm.splitDividerX ?? 0
  }}
/>

<style>
  .connection {
    padding: 2px 10px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.35);
    color: #fff;
    font: var(--type-caption1);
    font-weight: 600;
  }
  .stage {
    position: absolute;
    inset: 0;
  }
  /* a window's own box is positioned against the stage: the slot around it takes no part in the layout */
  .slot {
    display: contents;
  }
  .slot.away {
    display: none;
  }
  /* The Home Screen: the grid of apps over the wallpaper, down to the "Search" capsule above the Dock. */
  .home {
    position: absolute;
    inset: 0;
    z-index: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding-bottom: 116px; /* the Dock's room (104) and some air above it */
    animation: home-in var(--duration) var(--ease-ios);
  }
  /* it comes back as the last window leaves (at once under Reduce Motion: base.css) */
  @keyframes home-in {
    from {
      opacity: 0;
    }
  }
  .home-apps {
    flex: 1;
    min-height: 0;
    width: 100%;
    display: flex;
    justify-content: center;
  }
  .widgets {
    position: absolute;
    top: 16px;
    right: 24px;
    display: flex;
    flex-direction: column;
    align-items: flex-end;
    gap: 20px;
    transition: opacity var(--duration) var(--ease-ios);
  }
  .widgets.obscured {
    opacity: 0;
  }
  .divider {
    position: absolute;
    top: 8px;
    bottom: 8px;
    z-index: 9000;
    display: flex;
    align-items: center;
  }
  .split-placeholder {
    position: absolute;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 12px;
    padding: 20px 12px 12px;
    border-radius: var(--radius-xl);
    background: rgb(255 255 255 / 0.12);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    border: 1px dashed rgb(255 255 255 / 0.5);
    color: #fff;
    font: var(--type-headline);
    z-index: 5;
  }
  .split-placeholder p {
    flex: none;
    margin: 0;
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
  .notice {
    position: absolute;
    top: 8px; /* moved down under the notification banners while there are any */
    left: 50%;
    transform: translateX(-50%);
    width: min(440px, calc(100% - 32px));
    z-index: 10001; /* above the windows and Slide Over */
  }
  .slideover-tab {
    position: absolute;
    right: 0;
    top: 40%;
    width: 8px;
    height: 72px;
    border: 0;
    border-radius: 4px 0 0 4px;
    background: rgb(255 255 255 / 0.7);
    box-shadow: 0 0 0 0.5px rgb(0 0 0 / 0.15);
    cursor: pointer;
    z-index: 10001;
  }
</style>
