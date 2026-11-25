<!-- @organism TelemetrySidebar — what there is to look at (the traces, with how many there are, and the logs), and the probes that make a trace to look at -->
<script lang="ts">
  import type { Probe, View } from '../lib/telemetry.svelte'

  interface Props {
    /** What is shown; nothing is marked when left out (a narrow window showing only this). */
    selected?: View
    /** How many traces were recorded; nothing is said before the first read. */
    count?: number
    /** The probe on its way, while one is. */
    probing?: Probe | null
    onselect: (view: View) => void
    onprobe: (probe: Probe) => void
  }

  let { selected, count, probing = null, onselect, onprobe }: Props = $props()

  const PROBES: { probe: Probe; heading: string; icon: string; color: string }[] = [
    { probe: 'answer', heading: 'Probe', icon: 'play', color: 'green' },
    { probe: 'fail', heading: 'Failing Probe', icon: 'xmark', color: 'red' },
    { probe: 'render', heading: 'Render Error', icon: 'exclamationmark.triangle', color: 'orange' }
  ]
</script>

<nav class="sidebar" aria-label="Telemetry">
  <ui-list variant="sidebar" selection="single" label="Views" data-testid="telemetry-views">
    <ui-section header="Telemetry">
      <ui-row
        heading="Traces"
        value={count === undefined ? undefined : String(count)}
        label={count === undefined ? 'Traces' : `Traces, ${count}`}
        icon="network"
        icon-color="teal"
        selected={selected === 'traces'}
        onpress={() => onselect('traces')}
        data-testid="telemetry-view-traces"
      ></ui-row>
      <ui-row heading="Logs" icon="doc" icon-color="indigo" selected={selected === 'logs'} onpress={() => onselect('logs')} data-testid="telemetry-view-logs"
      ></ui-row>
    </ui-section>
  </ui-list>
  <ui-list variant="sidebar" label="Try It">
    <ui-section header="Try It" footer="Each is a call from this window to its backend, traced from the page to the host and back. Render Error then fails to draw what came back.">
      {#each PROBES as p (p.probe)}
        <ui-row
          tappable
          heading={p.heading}
          icon={p.icon}
          icon-color={p.color}
          disabled={probing !== null}
          onpress={() => onprobe(p.probe)}
          data-testid="telemetry-probe-{p.probe}"
        >
          {#if probing === p.probe}<ui-spinner slot="trailing" label="Sending"></ui-spinner>{/if}
        </ui-row>
      {/each}
    </ui-section>
  </ui-list>
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    height: 100%;
    overflow: auto;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
  }
</style>
