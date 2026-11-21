<!--
  @template ScreenTemplate — a single scrolling screen: navigation bar on top,
  scrollable content, optional bottom bar (tab bar / toolbar / composer).
  With `onrefresh`, a touch pull-down at the top refreshes the screen (the spinner shows until it resolves), and a
  "Refresh" button appears on focus for keyboard and screen reader users. Test id: `${testid}-refresh`.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import RefreshKey from '../molecules/RefreshKey.svelte'
  import { PULL_THRESHOLD } from '../logic/gestures'
  import { pullToRefresh } from '../logic/pullToRefresh'

  interface Props {
    testid?: string
    background?: 'grouped' | 'plain'
    navigationBar?: Snippet
    content: Snippet
    bottomBar?: Snippet
    onrefresh?: () => void | Promise<void>
  }

  let { testid, background = 'grouped', navigationBar, content, bottomBar, onrefresh }: Props = $props()

  let pulled = $state(0)
  let refreshing = $state(false)

  async function run(): Promise<void> {
    if (refreshing || !onrefresh) return
    refreshing = true
    try {
      await onrefresh()
    } finally {
      refreshing = false
    }
  }
</script>

<div class="screen {background}">
  <div
    class="scroll"
    class:has-bar={!!navigationBar}
    use:pullToRefresh={{
      enabled: !!onrefresh,
      onrefresh: run,
      onchange: (d, r) => {
        pulled = d
        refreshing = r
      }
    }}
  >
    {#if onrefresh}
      <RefreshKey testid="{testid ?? 'screen'}-refresh" onclick={run} disabled={refreshing} />
    {/if}
    {#if navigationBar}{@render navigationBar()}{/if}
    {#if onrefresh}
      <div class="pull" style:height="{refreshing ? PULL_THRESHOLD : pulled}px" aria-hidden="true">
        {#if pulled > 0 || refreshing}<span class="spinner" class:spin={refreshing}></span>{/if}
      </div>
    {/if}
    <div class="content">{@render content()}</div>
  </div>
  {#if bottomBar}<div class="bottom">{@render bottomBar()}</div>{/if}
</div>

<style>
  .screen {
    display: flex;
    flex-direction: column;
    width: 100%;
    height: 100%;
    min-height: 0;
  }
  .grouped {
    background: var(--bg-grouped-elevated);
  }
  .plain {
    background: var(--bg-elevated);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    overscroll-behavior-y: contain;
  }
  /* The navigation bar sticks to the top (--nav-bar-height): a row scrolled to the top stops below it. */
  .has-bar {
    scroll-padding-top: var(--nav-bar-height);
  }
  .content {
    padding-bottom: 24px;
  }
  .bottom {
    flex: none;
  }
  .pull {
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
    transition: height var(--duration-fast) var(--ease-ios);
  }
  .spinner {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 2px solid var(--label-quaternary);
    border-top-color: var(--label-secondary);
  }
  .spin {
    animation: spin 0.8s linear infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  @media (prefers-reduced-motion: reduce) {
    .pull {
      transition: none;
    }
    .spin {
      animation-duration: 2s !important; /* it says the app is busy: slower, but turning (base.css stops the rest) */
    }
  }
</style>
