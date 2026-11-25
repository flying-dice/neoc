<!-- @page ProcessesPage — the whole app: the views and how often the machine is read beside the processes (or the machine as a whole), the process that was picked looked at in the next column, and the question before one is ended; driven by one window's Processes state -->
<script lang="ts">
  import { untrack } from 'svelte'
  import OverviewPane from '../organisms/OverviewPane.svelte'
  import ProcessInspector from '../organisms/ProcessInspector.svelte'
  import ProcessList from '../organisms/ProcessList.svelte'
  import UsageTiles from '../organisms/UsageTiles.svelte'
  import ViewSidebar from '../organisms/ViewSidebar.svelte'
  import type { Processes } from '../lib/processes.svelte'

  interface Props {
    /** This window's state; the composition root builds it with the real `api` and `host`, a story with a fake. */
    processes: Processes
  }

  let { processes }: Props = $props()

  // in a window too narrow for all the columns, the split view shows one: the kit says when, the same for every app
  let narrow = $state(false)
  // whether the page is in front: the browser's own tab is showing it (`document.hidden`) and the shell has it on the
  // stage. A window the shell minimizes or sends away is not `document.hidden` (its frame is kept, with `display: none`),
  // but it has no box on screen, which is what the observer says
  let root = $state<HTMLElement>()
  let tabShowing = $state(!document.hidden)
  let onStage = $state(true)
  const showing = $derived(tabShowing && onStage)
  const overview = $derived(processes.overview)
  const detail = $derived(processes.detail)

  $effect(() => {
    untrack(() => void processes.load())
    return processes.listen()
  })

  // the window's choice is the backend's to hold only while the window is here: it is let go when the window closes
  $effect(() => {
    const leaving = (): void => processes.close()
    window.addEventListener('pagehide', leaving)
    return () => {
      window.removeEventListener('pagehide', leaving)
      leaving()
    }
  })

  // the machine is read again as often as the person chose, while the window is in front: a hidden window reads nothing
  $effect(() => {
    const seconds = processes.interval
    if (seconds === 0 || !showing) return
    const timer = setInterval(() => void processes.refresh(), seconds * 1000)
    return () => clearInterval(timer)
  })

  // what happened while the window was away is read as soon as it is back
  let wasShowing = true
  $effect(() => {
    const now = showing
    if (now && !wasShowing) untrack(() => void processes.refresh())
    wasShowing = now
  })

  $effect(() => {
    const changed = (): void => {
      tabShowing = !document.hidden
    }
    document.addEventListener('visibilitychange', changed)
    return () => document.removeEventListener('visibilitychange', changed)
  })

  $effect(() => {
    if (!root || typeof IntersectionObserver === 'undefined') return
    const watcher = new IntersectionObserver((entries) => {
      const latest = entries.at(-1)
      if (latest) onStage = latest.isIntersecting
    })
    watcher.observe(root)
    return () => watcher.disconnect()
  })
</script>

<main class="app" bind:this={root} data-testid="processes-app" data-showing={showing}>
  <ui-split-view column={processes.pane} oncollapsedchange={(e) => (narrow = e.detail.collapsed)}>
    <div slot="sidebar">
      <ViewSidebar
        selected={narrow ? undefined : processes.view}
        count={overview?.processCount}
        interval={processes.interval}
        updatedAt={processes.updatedAt}
        onselect={(view) => processes.show(view)}
        oninterval={(interval) => processes.every(interval)}
      />
    </div>
    {#if processes.view === 'processes'}
      <div slot="content">
        {#if overview}
          <ProcessList
            {overview}
            rows={processes.rows}
            total={processes.processes.length}
            query={processes.query}
            sort={processes.sort}
            selected={narrow ? null : (processes.choosing ?? processes.selected)}
            back={narrow}
            onsearch={(text) => processes.search(text)}
            onsort={(sort) => processes.order(sort)}
            onopen={(row) => void processes.choose(row)}
            onback={() => processes.back()}
          >
            {#snippet tiles()}<UsageTiles {overview} />{/snippet}
          </ProcessList>
        {/if}
      </div>
    {/if}
    <div>
      {#if !processes.loaded}
        <div class="waiting">
          {#if !processes.failure}<ui-spinner size="large" label="Reading the machine" data-testid="processes-loading"></ui-spinner>{/if}
        </div>
      {:else if overview && processes.view === 'overview'}
        <OverviewPane {overview} back={narrow} onback={() => processes.back()} />
      {:else if overview && detail}
        <ProcessInspector
          {detail}
          canTerminate={overview.canTerminate}
          endable={processes.endable}
          unavailable={processes.unavailable}
          ending={processes.ending}
          asking={processes.asking}
          refused={processes.refused}
          notice={processes.notice}
          back={narrow}
          onback={() => processes.back()}
          onask={(ending) => processes.ask(ending)}
          onconfirm={() => void processes.confirm()}
          onkeep={() => processes.keep()}
          onfollow={(related) => void processes.choose(related)}
        />
      {:else}
        <div class="waiting">
          <ui-empty-state
            icon="rectangle.stack"
            heading="No Process Selected"
            message="Pick a process to see its details and end it."
            data-testid="processes-none"
          ></ui-empty-state>
        </div>
      {/if}
    </div>
  </ui-split-view>
  <!-- why the last call failed (the host's sentence or the backend's), whichever column is in front; why a process was not ended is said at the process -->
  {#if processes.failure}
    <ui-banner tone="error" data-testid="processes-failure">{processes.failure}</ui-banner>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
    --ui-split-sidebar-width: 220px;
    --ui-split-content-width: 360px;
  }
  ui-split-view {
    flex: 1;
    min-height: 0;
  }
  .waiting {
    height: 100%;
    display: grid;
    place-items: center;
    padding: 16px;
    background: var(--bg-grouped-elevated);
  }
</style>
