<!--
  @template DesktopTemplate — the virtual-desktop layout: wallpaper, status bar,
  a free-form workspace (strip, windows, widgets) and a floating dock. Layout
  only — every region is a snippet filled by a page. The dock can auto-hide
  (full screen / split) and reappears while the pointer is at the bottom edge.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    /** Any CSS background for the wallpaper. */
    wallpaper?: string
    statusBar: Snippet
    workspace: Snippet
    dock: Snippet
    /** Above everything: sheets, alerts, overlays. */
    overlay?: Snippet
    dockHidden?: boolean
    /** Bound to the workspace size, for layout maths. */
    workspaceWidth?: number
    workspaceHeight?: number
  }

  let {
    wallpaper = 'var(--wallpaper-default)',
    statusBar,
    workspace,
    dock,
    overlay,
    dockHidden = false,
    workspaceWidth = $bindable(0),
    workspaceHeight = $bindable(0)
  }: Props = $props()

  let peeking = $state(false)
</script>

<div class="desktop" style:--wallpaper={wallpaper}>
  <div class="status-bar">{@render statusBar()}</div>
  <main class="workspace" bind:clientWidth={workspaceWidth} bind:clientHeight={workspaceHeight}>
    {@render workspace()}
  </main>
  {#if dockHidden}
    <div class="dock-edge" role="presentation" onpointerenter={() => (peeking = true)}></div>
  {/if}
  <div
    class="dock"
    class:hidden={dockHidden && !peeking}
    role="presentation"
    onpointerleave={() => (peeking = false)}
  >
    {@render dock()}
  </div>
  {#if overlay}{@render overlay()}{/if}
</div>

<style>
  .desktop {
    --wallpaper-default:
      radial-gradient(120% 90% at 10% 0%, #6e8efb 0%, transparent 55%),
      radial-gradient(90% 80% at 100% 10%, #c471ed 0%, transparent 55%),
      radial-gradient(100% 90% at 80% 100%, #f7797d 0%, transparent 55%),
      radial-gradient(90% 90% at 0% 100%, #12c2e9 0%, transparent 60%), #2a2d6b;
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--wallpaper);
    background-size: cover;
    isolation: isolate;
  }
  .status-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 900;
    padding-top: 6px;
  }
  .workspace {
    position: absolute;
    inset: 34px 0 0;
  }
  .dock {
    position: absolute;
    left: 50%;
    bottom: 12px;
    transform: translateX(-50%);
    z-index: 11000;
    transition:
      transform var(--duration-slow) var(--ease-ios),
      opacity var(--duration-slow) var(--ease-ios);
  }
  .dock.hidden {
    transform: translate(-50%, 140%);
    opacity: 0;
    pointer-events: none;
  }
  .dock-edge {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: 10px;
    z-index: 11001;
  }
</style>
