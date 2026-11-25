<!-- @organism ViewSidebar — what there is to look at (the machine as a whole, or its processes, with how many there are), and how often the machine is read -->
<script lang="ts">
  import { clockTime } from '../lib/format'
  import type { Interval, View } from '../lib/processes.svelte'

  interface Props {
    /** What is shown; nothing is marked when left out (a narrow window showing only this). */
    selected?: View
    /** How many processes there are; nothing is said before the first reading. */
    count?: number
    interval: Interval
    /** When the machine was last read, in milliseconds since the epoch. */
    updatedAt?: number | null
    onselect: (view: View) => void
    oninterval: (interval: Interval) => void
  }

  let { selected, count, interval, updatedAt = null, onselect, oninterval }: Props = $props()

  const EVERY = [
    { value: '1', label: '1 Second' },
    { value: '2', label: '2 Seconds' },
    { value: '5', label: '5 Seconds' },
    { value: '0', label: 'Paused' }
  ]
</script>

<nav class="sidebar" aria-label="Monitor">
  <ui-list variant="sidebar" selection="single" label="Views" data-testid="processes-views">
    <ui-section header="Monitor">
      <ui-row
        heading="Overview"
        icon="cpu"
        icon-color="blue"
        selected={selected === 'overview'}
        onpress={() => onselect('overview')}
        data-testid="processes-view-overview"
      ></ui-row>
      <ui-row
        heading="Processes"
        value={count === undefined ? undefined : String(count)}
        label={count === undefined ? 'Processes' : `Processes, ${count}`}
        icon="rectangle.stack"
        icon-color="indigo"
        selected={selected === 'processes'}
        onpress={() => onselect('processes')}
        data-testid="processes-view-processes"
      ></ui-row>
    </ui-section>
  </ui-list>
  <ui-list variant="sidebar" label="Refresh">
    <ui-section
      header="Refresh"
      footer="{updatedAt === null ? 'Not read yet.' : `Updated ${clockTime(updatedAt)}.`} Paused while this window is in the background."
    >
      <ui-row heading="Every">
        <ui-picker
          slot="trailing"
          variant="bare"
          label="Refresh interval"
          value={String(interval)}
          options={EVERY}
          onchange={(e) => oninterval(Number(e.detail.value) as Interval)}
          data-testid="processes-interval"
        ></ui-picker>
      </ui-row>
    </ui-section>
  </ui-list>
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 36px;
    overflow: auto;
    background: var(--bg-elevated);
  }
</style>
