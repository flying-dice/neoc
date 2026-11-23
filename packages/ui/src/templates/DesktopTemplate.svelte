<!--
  @template DesktopTemplate — the virtual-desktop layout: wallpaper, status bar,
  a free-form workspace (strip, windows, widgets) and a floating dock. Layout
  only — every region is a snippet filled by a page. The dock can auto-hide
  (full screen / split) and reappears while the pointer is at the bottom edge.
  `overlay` is a layer over all of it (the status bar and the Dock too) for what the system presents: a context
  menu, system search. It takes no presses itself; what is put in it does.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    statusBar: Snippet
    workspace: Snippet
    dock: Snippet
    /** Drawn over everything else, in a box the size of the desktop. */
    overlay?: Snippet
    dockHidden?: boolean
    /** Bound to the workspace size, for layout maths. */
    workspaceWidth?: number
    workspaceHeight?: number
  }

  let {
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

<div class="desktop">
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
  {#if overlay}<div class="overlay">{@render overlay()}</div>{/if}
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
    /* `clip`, not `hidden`: a hidden box can still be scrolled by script, and focusing or scrolling into view something a
       window left past the edge (an icon of the Dock) would slide the whole desktop sideways under the status bar */
    overflow: clip;
    background: var(--wallpaper-default);
    background-size: cover;
    isolation: isolate;
  }
  .status-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 900;
    /* the safe areas: on a phone the desktop is drawn edge to edge, under the device's own status bar and home indicator */
    padding-top: calc(6px + env(safe-area-inset-top, 0px));
  }
  .workspace {
    position: absolute;
    inset: calc(34px + env(safe-area-inset-top, 0px)) 0 0;
  }
  .dock {
    position: absolute;
    left: 50%;
    bottom: max(12px, env(safe-area-inset-bottom, 0px));
    transform: translateX(-50%);
    /* as wide as its icons, but never wider than the screen: `left: 50%` alone would give it half the screen to fit in, and
       icons past the right edge could not be reached (the tray wraps instead, see Dock) */
    width: max-content;
    max-width: calc(100% - 24px);
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
  .overlay {
    position: absolute;
    inset: 0;
    z-index: 12000; /* above the Dock */
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
