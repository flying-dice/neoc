<!--
  @template SplitViewTemplate — UISplitViewController (sidebar + detail), the
  iPadOS Settings / Mail layout. Collapses to detail-only below `collapseAt`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    sidebarWidth?: number
    /** Container width (px) below which the sidebar is hidden. */
    collapseAt?: number
    sidebar: Snippet
    detail: Snippet
  }

  let { sidebarWidth = 320, collapseAt = 560, sidebar, detail }: Props = $props()

  let width = $state(0)
  const collapsed = $derived(width > 0 && width < collapseAt)
</script>

<div class="split" bind:clientWidth={width} style:--sidebar-width="{sidebarWidth}px">
  {#if !collapsed}
    <aside class="sidebar">{@render sidebar()}</aside>
  {/if}
  <section class="detail">{@render detail()}</section>
</div>

<style>
  .split {
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .sidebar {
    flex: none;
    width: min(var(--sidebar-width), 40%);
    overflow-y: auto;
    background: var(--bg-grouped-elevated);
    box-shadow: var(--hairline) 0 0 var(--separator);
    z-index: 1;
  }
  .detail {
    flex: 1;
    min-width: 0;
    overflow-y: auto;
    background: var(--bg-grouped-elevated);
    --list-row-bg: var(--bg-grouped-elevated-secondary);
  }
</style>
