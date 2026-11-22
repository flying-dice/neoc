<!--
  @page DesktopPage — the vos desktop, iPadOS Stage Manager style: status
  bar, recent-apps strip, the current stage (floating / full screen / split
  windows), Slide Over, widgets and the dock. On a compact screen (a phone) every window is shown full screen above
  the Dock, one app at a time (`WindowManager.compact`). Window contents are supplied by
  the composition root through the `windowContent` snippet.

  Test ids: `status-bar`, `desktop-stage`, `widget-clock`, `widget-agent-activity`,
  `stage-strip` (+ `stage-strip-<appIds>`), `window-${appId}` (+ parts, see Window),
  `split-divider`, `split-placeholder`, `slideover-tab`, `dock` (+ `dock-${appId}`), `desktop-notice` (+ `-dismiss`), and the system alerts (see NotificationBanners).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { DockApp } from '@vos/ui'
  import { SplitHandle, Toast } from '@vos/ui/atoms'
  import { DesktopTemplate } from '@vos/ui/templates'
  import { StatusBar, Dock, Window, StageStrip } from '@vos/ui/organisms'
  import ClockWidget from '../organisms/ClockWidget.svelte'
  import AgentActivityWidget from '../organisms/AgentActivityWidget.svelte'
  import NotificationBanners from '../organisms/NotificationBanners.svelte'
  import {
    AGENTS_APP_ID,
    appOf,
    dockOrder,
    dockRecents,
    installedApp,
    onAppsRemoved,
    type AppId
  } from '../lib/apps.svelte'
  import { windowManager as wm, type WindowState } from '../lib/windowManager.svelte'
  import { host } from '../lib/host.svelte'
  import { listenToHost, notifications } from '../lib/notifications.svelte'

  interface Props {
    windowContent: Snippet<[WindowState]>
  }

  let { windowContent }: Props = $props()

  // What an app's backend tells the person (`notification.deliver`) is drawn here, above every window; an app that
  // stops running takes its alerts with it.
  let bannersHeight = $state(0)
  $effect(() => listenToHost(host))
  $effect(() => onAppsRemoved((ids) => notifications.forgetApps(ids)))

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

  function toDock(id: AppId): DockApp[] {
    const app = appOf(id)
    return app
      ? [{ id, name: app.name, icon: app.icon, background: app.background, running: wm.isOpen(id) }]
      : []
  }

  const dockApps = $derived(dockOrder().flatMap(toDock))
  const recentApps = $derived(dockRecents.flatMap(toDock))
  // The Agents app is an ordinary installed app: the widget offers to open it only while it is running.
  const agentsRunning = $derived(!!appOf(AGENTS_APP_ID))
  const awaitingSplit = $derived(wm.activeStage?.awaitingSplit ?? null)

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
  const widgetsObscured = $derived(
    [...wm.onStage, ...(wm.slideOver && !wm.slideOverHidden ? [wm.slideOver] : [])].some((w) => {
      const f = wm.frameOf(w)
      return f.x + f.width > wm.bounds.width - 24 - widgetsWidth && f.y < 16 + widgetsHeight
    })
  )
</script>

<DesktopTemplate bind:workspaceWidth bind:workspaceHeight dockHidden={wm.dockHidden}>
  {#snippet statusBar()}
    <div data-testid="status-bar">
      <StatusBar battery={0.86}>
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

      <div
        class="widgets"
        class:obscured={widgetsObscured}
        aria-hidden={widgetsObscured}
        inert={widgetsObscured}
        bind:clientWidth={widgetsWidth}
        bind:clientHeight={widgetsHeight}
      >
        <ClockWidget testid="widget-clock" />
        <AgentActivityWidget
          testid="widget-agent-activity"
          agents={host.agents}
          onopen={agentsRunning ? () => wm.open(AGENTS_APP_ID) : undefined}
        />
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
        <!-- An app with a client of its own keeps its page for as long as its window exists: off the active stage the window is
             kept in the page, hidden (`display: none` keeps the frame's document: it is not reloaded when shown again, takes
             no focus or pointer events and is not read by a screen reader), so what the client holds in its page survives
             switching apps. The windows of the other apps are drawn again from the host's state when their stage returns. -->
        {#if app && (onStage || installedApp(win.appId)?.client)}
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
          <p>Choose an app from the Dock</p>
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
      apps={dockApps}
      recents={recentApps}
      onselect={(id) => wm.open(id as AppId)}
    />
  {/snippet}
</DesktopTemplate>

<svelte:window
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
    display: grid;
    place-items: center;
    border-radius: var(--radius-xl);
    background: rgb(255 255 255 / 0.12);
    -webkit-backdrop-filter: blur(20px);
    backdrop-filter: blur(20px);
    border: 1px dashed rgb(255 255 255 / 0.5);
    color: #fff;
    font: var(--type-headline);
    z-index: 5;
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
