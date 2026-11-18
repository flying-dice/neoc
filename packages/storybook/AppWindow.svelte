<!--
  Story frame for app content: the same surface, radius, shadow and insets as
  a desktop Window, on the desktop wallpaper, at an exact content size.

  Test ids: `app-window` (the frame) — the stories spec measures overflow
  against it and runs resize checks on stories tagged `app` through the
  `width` / `height` args.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    /** Window title, used as the frame's accessible name. */
    title: string
    width?: number
    height?: number
    /** The app's minimum window size (from its manifest); width/height are clamped to it, like a real Window. */
    minWidth?: number
    minHeight?: number
    children: Snippet
  }

  let { title, width = 720, height = 520, minWidth = 320, minHeight = 240, children }: Props = $props()
</script>

<div class="wallpaper">
  <div
    class="window"
    data-testid="app-window"
    role="region"
    aria-label={title}
    style:width="{Math.max(width, minWidth)}px"
    style:height="{Math.max(height, minHeight)}px"
  >
    <div class="content">{@render children()}</div>
  </div>
</div>

<style>
  .wallpaper {
    padding: 24px;
    background:
      radial-gradient(120% 90% at 10% 0%, #6e8efb 0%, transparent 55%),
      radial-gradient(90% 80% at 100% 10%, #c471ed 0%, transparent 55%),
      radial-gradient(100% 90% at 80% 100%, #f7797d 0%, transparent 55%),
      radial-gradient(90% 90% at 0% 100%, #12c2e9 0%, transparent 60%), #2a2d6b;
  }
  /* Mirrors packages/ui/src/organisms/Window.svelte. */
  .window {
    position: relative;
    display: flex;
    flex-direction: column;
    border-radius: var(--radius-xl);
    background: var(--bg-grouped-elevated);
    color: var(--label);
    box-shadow: var(--shadow-window);
  }
  .content {
    flex: 1;
    min-height: 0;
    overflow: hidden;
    border-radius: inherit;
    display: flex;
    flex-direction: column;
    --nav-top-inset: 10px;
  }
  .content > :global(*) {
    flex: 1;
    min-height: 0;
  }
</style>
