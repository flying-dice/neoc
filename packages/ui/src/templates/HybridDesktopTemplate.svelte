<!--
  @template HybridDesktopTemplate — macOS / iPadOS hybrid desktop: a menu bar
  across the top, a free-form workspace (windows + iPad-style widgets) and a
  dock along the bottom. Layout only; every region is a snippet.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    wallpaper?: string
    menuBar: Snippet
    workspace: Snippet
    dock: Snippet
    overlay?: Snippet
    /** Bound to the workspace size, for zoom / placement maths. */
    workspaceWidth?: number
    workspaceHeight?: number
  }

  let {
    wallpaper = 'var(--wallpaper-hybrid)',
    menuBar,
    workspace,
    dock,
    overlay,
    workspaceWidth = $bindable(0),
    workspaceHeight = $bindable(0)
  }: Props = $props()
</script>

<div class="desktop" style:--wallpaper={wallpaper}>
  <div class="menu-bar">{@render menuBar()}</div>
  <main class="workspace" bind:clientWidth={workspaceWidth} bind:clientHeight={workspaceHeight}>
    {@render workspace()}
  </main>
  <div class="dock">{@render dock()}</div>
  {#if overlay}{@render overlay()}{/if}
</div>

<style>
  .desktop {
    /* Layered "coastal dusk" wallpaper — blue sky, magenta and amber hills. */
    --wallpaper-hybrid:
      radial-gradient(140% 70% at 100% 85%, #ffb347 0%, #ff7e5f 22%, transparent 48%),
      radial-gradient(120% 80% at 0% 90%, #b4235a 0%, #e8336d 30%, transparent 60%),
      radial-gradient(90% 60% at 20% 45%, #ff4d6d 0%, transparent 60%),
      radial-gradient(120% 70% at 85% 10%, #5fb0ff 0%, transparent 60%),
      linear-gradient(170deg, #1e6fd9 0%, #4a6fe0 35%, #8a3fbf 65%, #1b2a6b 100%);
    position: relative;
    width: 100%;
    height: 100%;
    overflow: hidden;
    background: var(--wallpaper);
    isolation: isolate;
  }
  .menu-bar {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    z-index: 900;
  }
  .workspace {
    position: absolute;
    inset: 26px 0 0;
  }
  .dock {
    position: absolute;
    left: 50%;
    bottom: 6px;
    transform: translateX(-50%);
    z-index: 800;
  }
</style>
