<!-- @organism ProcessList — the processes of the machine: how busy it is at the top, what to look for and in what order, and a row for each process, the one that is shown on the right marked -->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import type { Overview, ProcessRef, ProcessRow } from '@processes/rpc'
  import ProcessRowItem from '../molecules/ProcessRow.svelte'
  import { processesSummary } from '../lib/format'
  import { same, type Sort } from '../lib/processes.svelte'

  interface Props {
    overview: Overview
    /** The processes to show: what matches the search, in the order chosen. */
    rows: readonly ProcessRow[]
    /** How many processes there are, before the search. */
    total: number
    query: string
    sort: Sort
    /** The process that is shown beside the list; nothing is marked when left out (a narrow window shows one thing). */
    selected?: ProcessRef | null
    /** Show the way back to the views (a window too narrow for both). */
    back?: boolean
    /** How busy the machine is, above the search: the page draws it (an organism does not hold another). */
    tiles?: Snippet
    onsearch: (text: string) => void
    onsort: (sort: Sort) => void
    onopen: (process: ProcessRow) => void
    onback?: () => void
  }

  let { overview, rows, total, query, sort, selected = null, back = false, tiles, onsearch, onsort, onopen, onback }: Props = $props()

  const SORTS = [
    { value: 'cpu', label: 'CPU' },
    { value: 'memory', label: 'Memory' },
    { value: 'pid', label: 'PID' },
    { value: 'name', label: 'Name' }
  ]
  const valueOf = (e: Event): string => (e.target as HTMLInputElement).value
</script>

<section class="list" aria-label="Processes">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Monitor" onclick={onback} data-testid="processes-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Monitor
      </ui-button>
    {/if}
    <h1 class="ios-large-title" data-testid="processes-title">Processes</h1>
  </header>
  <div class="scroll">
    {@render tiles?.()}
    <div class="controls">
      <ui-text-field
        type="search"
        variant="field"
        placeholder="Search by name or PID"
        label="Search processes"
        inputmode="search"
        enterkeyhint="search"
        autocapitalize="off"
        autocomplete="off"
        value={query}
        oninput={(e) => onsearch(valueOf(e))}
        data-testid="processes-search"
      ></ui-text-field>
      <ui-segmented
        block
        label="Sort by"
        value={sort}
        options={SORTS}
        onchange={(e) => onsort(e.detail.value as Sort)}
        data-testid="processes-sort"
      ></ui-segmented>
    </div>
    {#if rows.length > 0}
      <ui-list variant="insetGrouped" selection="single" label="Processes" data-testid="processes-items">
        <ui-section
          header={query.trim() === '' ? processesSummary(total) : `${rows.length} of ${processesSummary(total)}`}
          footer={overview.cpu.sampled ? 'CPU is a share of one core, so it can pass 100%.' : 'CPU needs two readings. It appears after the next refresh.'}
        >
          {#each rows as row (`${row.pid}:${row.startTime}:${row.observation}`)}
            <ProcessRowItem process={row} selected={same(row, selected)} onpress={() => onopen(row)} />
          {/each}
        </ui-section>
      </ui-list>
    {:else}
      <ui-empty-state
        icon="magnifyingglass"
        heading={total === 0 ? 'No Processes' : 'No Matching Processes'}
        message={total === 0 ? 'This machine did not report any.' : `Nothing matches “${query.trim()}”. Check the name or PID.`}
        data-testid="processes-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .list {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
  }
  .bar {
    flex: none;
    padding: 4px 16px 8px 20px;
  }
  .bar h1 {
    margin: 0;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow: auto;
  }
  .controls {
    display: flex;
    flex-direction: column;
    gap: 10px;
    padding: 0 16px 8px;
  }
</style>
