<!--
  @template MacSplitViewTemplate — macOS app layout: a translucent sidebar
  (with room for the traffic lights) beside a unified toolbar and content.
  Use inside a `vibrant` MacWindow. Sidebar top and toolbar are drag regions.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    sidebarWidth?: number
    sidebar: Snippet
    sidebarFooter?: Snippet
    toolbar?: Snippet
    content: Snippet
  }

  let { sidebarWidth = 220, sidebar, sidebarFooter, toolbar, content }: Props = $props()
</script>

<div class="mac-split" style:--sidebar-width="{sidebarWidth}px">
  <aside class="sidebar">
    <div class="sidebar-head" data-drag-region></div>
    <div class="sidebar-body">{@render sidebar()}</div>
    {#if sidebarFooter}<div class="sidebar-footer">{@render sidebarFooter()}</div>{/if}
  </aside>
  <section class="main">
    <header class="toolbar" data-drag-region>
      {#if toolbar}{@render toolbar()}{/if}
    </header>
    <div class="scroll">{@render content()}</div>
  </section>
</div>

<style>
  .mac-split {
    display: flex;
    width: 100%;
    height: 100%;
    border-radius: inherit;
    overflow: hidden;
  }
  .sidebar {
    flex: none;
    width: var(--sidebar-width);
    display: flex;
    flex-direction: column;
    background: light-dark(rgb(246 240 248 / 0.62), rgb(40 36 48 / 0.62));
    -webkit-backdrop-filter: blur(50px) saturate(200%);
    backdrop-filter: blur(50px) saturate(200%);
    box-shadow: inset -0.5px 0 0 var(--separator);
  }
  :global(:root.reduce-transparency) .sidebar {
    background: var(--bg-secondary);
  }
  .sidebar-head {
    flex: none;
    height: 50px;
  }
  .sidebar-body {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 10px 10px;
  }
  .sidebar-footer {
    flex: none;
    padding: 10px 14px 12px;
  }
  .main {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    background: var(--bg-elevated);
  }
  .toolbar {
    flex: none;
    display: flex;
    align-items: center;
    gap: 10px;
    height: 52px;
    padding: 0 16px;
    background: var(--bg-elevated);
    box-shadow: 0 0.5px 0 var(--separator);
    z-index: 1;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
  }
</style>
