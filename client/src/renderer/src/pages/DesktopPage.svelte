<!--
  @page DesktopPage — the AgentOS desktop, iPadOS Stage Manager style: status
  bar, recent-apps strip, the current stage (floating / full screen / split
  windows), Slide Over, widgets and the dock. Window contents are supplied by
  the composition root through the `windowContent` snippet.

  Test ids: `status-bar`, `desktop-stage`, `widget-clock`, `widget-agent-activity`,
  `stage-strip` (+ `stage-strip-<appIds>`), `window-${appId}` (+ parts, see Window),
  `split-divider`, `split-placeholder`, `slideover-tab`, `dock` (+ `dock-${appId}`).
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { DockApp } from '@agentos/ui'
  import { SplitHandle } from '@agentos/ui/atoms'
  import { DesktopTemplate } from '@agentos/ui/templates'
  import { StatusBar, Dock, Window, StageStrip } from '@agentos/ui/organisms'
  import ClockWidget from '../organisms/ClockWidget.svelte'
  import AgentActivityWidget from '../organisms/AgentActivityWidget.svelte'
  import { apps, dockOrder, dockRecents, type AppId } from '../lib/apps.svelte'
  import { windowManager as wm, type WindowState } from '../lib/windowManager.svelte'
  import { host } from '../lib/host.svelte'

  interface Props {
    windowContent: Snippet<[WindowState]>
  }

  let { windowContent }: Props = $props()

  let workspaceWidth = $state(0)
  let workspaceHeight = $state(0)

  $effect(() => {
    if (workspaceWidth && workspaceHeight)
      wm.bounds = { width: workspaceWidth, height: workspaceHeight }
  })

  // The link to the host: say so in the status bar while it's down.
  const connection = $derived(host.connection)

  function toDock(id: AppId): DockApp {
    const app = apps[id]
    return {
      id,
      name: app.name,
      icon: app.icon,
      background: app.background,
      running: wm.isOpen(id)
    }
  }

  const dockApps = $derived(dockOrder().map(toDock))
  const recentApps = $derived(dockRecents.map(toDock))
  const awaitingSplit = $derived(wm.activeStage?.awaitingSplit ?? null)

  // Dragging a strip group onto the stage merges it into the current group.
  function ondragover(e: DragEvent): void {
    if (e.dataTransfer?.types.includes('application/x-agentos-stage')) e.preventDefault()
  }
  function ondrop(e: DragEvent): void {
    const id = e.dataTransfer?.getData('application/x-agentos-stage')
    if (id) {
      e.preventDefault()
      wm.mergeStage(id)
    }
  }

  let dividerStart = 0

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
                ? 'Connecting to AgentOS…'
                : connection === 'incompatible'
                  ? 'Reload to update AgentOS'
                  : 'Reconnecting to AgentOS…'}
            </span>
          {/if}
        {/snippet}
      </StatusBar>
    </div>
  {/snippet}

  {#snippet workspace()}
    <div class="stage" role="presentation" data-testid="desktop-stage" {ondragover} {ondrop}>
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
          onopen={() => wm.open('agents')}
        />
      </div>

      <StageStrip
        testid="stage-strip"
        stages={wm.recentStages}
        autoHide={wm.stripObscured}
        onselect={(id) => wm.activateStage(id)}
      />

      {#each wm.windows as win (win.id)}
        {#if win.stageId === wm.activeStageId || win.layout === 'slideover'}
          {@const frame = wm.frameOf(win)}
          <Window
            testid="window-{win.appId}"
            title={apps[win.appId].name}
            x={frame.x}
            y={frame.y}
            width={frame.width}
            height={frame.height}
            minWidth={apps[win.appId].minWidth}
            minHeight={apps[win.appId].minHeight}
            z={win.layout === 'slideover' ? 10000 : win.z}
            layout={win.layout}
            focused={wm.focusedId === win.id}
            onframechange={(f) => wm.setFrame(win.id, f)}
            onfocus={() => wm.focus(win.id)}
            onlayout={(l) => wm.setLayout(win.id, l)}
            onminimize={() => wm.minimize(win.id)}
            onclose={() => wm.close(win.id)}
          >
            {@render windowContent(win)}
          </Window>
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

      {#if awaitingSplit}
        <div class="split-placeholder {awaitingSplit}" data-testid="split-placeholder">
          <p>Choose an app from the Dock</p>
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
    top: 8px;
    bottom: 8px;
    width: calc(50% - 13px);
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
  .split-placeholder.split-left {
    left: 8px;
  }
  .split-placeholder.split-right {
    right: 8px;
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
