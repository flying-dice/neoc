<!--
  Story helper (not part of the kit): a positioned, sized frame so absolutely
  positioned overlays (Sheet, Alert, Window, templates) have a container.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    width?: number | string
    height?: number | string
    wallpaper?: boolean
    padding?: string
    children: Snippet
  }

  let { width = 720, height = 460, wallpaper = false, padding = '0', children }: Props = $props()
  const px = (v: number | string): string => (typeof v === 'number' ? `${v}px` : v)
</script>

<div class="stage" class:wallpaper style:width={px(width)} style:height={px(height)} style:padding>
  {@render children()}
</div>

<style>
  .stage {
    position: relative;
    overflow: hidden;
    border-radius: 18px;
    background: var(--bg-grouped);
    box-shadow: 0 0 0 0.5px var(--separator);
    max-width: calc(100vw - 32px);
  }
  .wallpaper {
    background:
      radial-gradient(120% 90% at 10% 0%, #6e8efb 0%, transparent 55%),
      radial-gradient(90% 80% at 100% 10%, #c471ed 0%, transparent 55%),
      radial-gradient(100% 90% at 80% 100%, #f7797d 0%, transparent 55%),
      radial-gradient(90% 90% at 0% 100%, #12c2e9 0%, transparent 60%), #2a2d6b;
  }
</style>
