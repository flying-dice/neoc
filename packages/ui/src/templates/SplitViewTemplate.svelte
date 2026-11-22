<!--
  @template SplitViewTemplate — UISplitViewController: sidebar + detail (the iPadOS Settings layout). In a container
  narrower than 560 px it collapses to ONE column, `compact`: the detail by default, or the sidebar, which the caller
  moves between as a navigation flow (the sidebar is the list, choosing a row shows the detail, Back returns).
  `collapsed` tells the caller which of the two layouts is on screen. The columns are `aside`/`section`; the content
  of each is the caller's.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    sidebarWidth?: number
    /** The column shown while collapsed. */
    compact?: 'sidebar' | 'detail'
    /** Out: whether the split is collapsed to one column right now. */
    collapsed?: boolean
    sidebar: Snippet
    detail: Snippet
  }

  let { sidebarWidth = 320, compact = 'detail', collapsed = $bindable(false), sidebar, detail }: Props = $props()

  /** Container width (px) below which the split collapses to one column. */
  const COLLAPSE_AT = 560

  let width = $state(0)
  const measure = (measured: number): void => {
    width = measured
    collapsed = measured > 0 && measured < COLLAPSE_AT
  }
</script>

<div class="split" class:collapsed bind:clientWidth={() => width, measure} style:--sidebar-width="{sidebarWidth}px">
  {#if !collapsed || compact === 'sidebar'}
    <aside class="sidebar">
      {@render sidebar()}
    </aside>
  {/if}
  {#if !collapsed || compact === 'detail'}
    <section class="detail">
      {@render detail()}
    </section>
  {/if}
</div>

<style>
  .split {
    position: relative;
    display: flex;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .sidebar,
  .detail {
    min-width: 0;
    overflow-y: auto;
    background: var(--bg-grouped-elevated);
    --list-row-bg: var(--bg-grouped-elevated-secondary);
  }
  .sidebar {
    flex: none;
    width: min(var(--sidebar-width), 40%);
    box-shadow: var(--hairline) 0 0 var(--separator);
    z-index: 2;
  }
  .detail {
    flex: 1;
  }
  /* Collapsed: the one column on screen fills the split. */
  .collapsed .sidebar,
  .collapsed .detail {
    position: absolute;
    inset: 0;
    width: auto;
    box-shadow: none;
  }
</style>
