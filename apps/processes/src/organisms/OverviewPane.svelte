<!-- @organism OverviewPane — the machine as a whole: each core's load, memory and swap, the disks and what they read and write, the network interfaces, and what the machine is -->
<script lang="ts">
  type Overview = VosApp.Processes.Overview
  import { bytes, duration, percent, rate, share, whole } from '../lib/format'

  interface Props {
    overview: Overview
    /** Show the way back to the views (a window too narrow for both). */
    back?: boolean
    onback?: () => void
  }

  let { overview, back = false, onback }: Props = $props()

  const cpu = $derived(overview.cpu)
  const rates = $derived(overview.disks.flatMap((disk) => (disk.readRate === undefined ? [] : [disk])))
  const read = $derived(rates.length === 0 ? undefined : rates.reduce((sum, d) => sum + (d.readRate ?? 0), 0))
  const written = $derived(rates.length === 0 ? undefined : rates.reduce((sum, d) => sum + (d.writeRate ?? 0), 0))
  const cores = $derived(cpu.logicalCores === 1 ? '1 core' : `${cpu.logicalCores} cores`)
  const loaded = (network: { receiveRate?: number; transmitRate?: number }): boolean => network.receiveRate !== undefined
  const machine = $derived(overview.machine)
  const os = $derived([machine.osName, machine.osVersion].filter((part) => part !== undefined && part !== '').join(' '))
</script>

<!-- the pane scrolls and has nothing to focus: the keyboard scrolls it -->
<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
<section class="overview" aria-label="Overview" tabindex="0" data-testid="processes-overview">
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to Monitor" onclick={onback} data-testid="processes-overview-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Monitor
      </ui-button>
    {/if}
    <h1 class="ios-large-title">Overview</h1>
  </header>
  <div class="grid">
    <ui-list variant="insetGrouped" label="CPU">
      <ui-section header="CPU" footer={cpu.sampled ? 'Load of each core since the last refresh.' : 'CPU needs two readings. It appears after the next refresh.'}>
        <ui-row data-testid="processes-overview-cpu">
          <div class="stack">
            <div class="line">
              <span class="ios-body">{cores}</span>
              <span class="ios-subheadline ios-secondary">{cpu.sampled ? `${whole(cpu.usage)} overall` : 'Not measured yet'}</span>
            </div>
            <div class="cores" role="group" aria-label="Load of each core">
              {#each cpu.cores as core, i (i)}
                <div class="core">
                  <span class="ios-caption1">{whole(core.usage)}</span>
                  <ui-meter orientation="vertical" label="Core {i + 1}" value={core.usage}></ui-meter>
                  <span class="ios-caption1 ios-secondary">{i + 1}</span>
                </div>
              {/each}
            </div>
          </div>
        </ui-row>
        {#if cpu.brand}<ui-row heading="Processor" value={cpu.brand}></ui-row>{/if}
        {#if cpu.loadAverage}
          <ui-row
            heading="Load Average"
            value="{cpu.loadAverage.one.toFixed(2)} · {cpu.loadAverage.five.toFixed(2)} · {cpu.loadAverage.fifteen.toFixed(2)}"
            data-testid="processes-overview-load"
          ></ui-row>
        {/if}
      </ui-section>
    </ui-list>

    <ui-list variant="insetGrouped" label="Memory">
      <ui-section header="Memory">
        <ui-row data-testid="processes-overview-memory">
          <div class="stack">
            <div class="line">
              <span class="ios-body">Memory</span>
              <span class="ios-subheadline ios-secondary">{bytes(overview.memory.used)} of {bytes(overview.memory.total)}</span>
            </div>
            <ui-meter label="Memory used" value={share(overview.memory.used, overview.memory.total)}></ui-meter>
          </div>
        </ui-row>
        <ui-row heading="Available" value={bytes(overview.memory.available)}></ui-row>
        <ui-row data-testid="processes-overview-swap">
          <div class="stack">
            <div class="line">
              <span class="ios-body">Swap</span>
              <span class="ios-subheadline ios-secondary">
                {overview.swap.total > 0 ? `${bytes(overview.swap.used)} of ${bytes(overview.swap.total)}` : 'None'}
              </span>
            </div>
            <ui-meter label="Swap used" value={share(overview.swap.used, overview.swap.total)}></ui-meter>
          </div>
        </ui-row>
      </ui-section>
    </ui-list>

    <ui-list variant="insetGrouped" label="Disks">
      <ui-section header="Disks" footer="Rates are totals across all disks since the last refresh.">
        {#each overview.disks as disk (disk.mountPoint)}
          <ui-row data-testid="processes-disk-{disk.mountPoint}">
            <div class="stack">
              <div class="line">
                <span class="ios-body">{disk.mountPoint}</span>
                <span class="ios-subheadline ios-secondary">{bytes(disk.totalBytes - disk.availableBytes)} of {bytes(disk.totalBytes)}</span>
              </div>
              <ui-meter label="Disk {disk.mountPoint} used" value={share(disk.totalBytes - disk.availableBytes, disk.totalBytes)}></ui-meter>
            </div>
          </ui-row>
        {/each}
        <ui-row heading="Read" value={rate(read)} data-testid="processes-disk-read"></ui-row>
        <ui-row heading="Write" value={rate(written)} data-testid="processes-disk-write"></ui-row>
      </ui-section>
    </ui-list>

    <ui-list variant="insetGrouped" label="Network">
      <ui-section header="Network" footer="Rates are since the last refresh.">
        {#each overview.networks as network (network.name)}
          <ui-row
            heading={network.name}
            subheading={loaded(network) ? `Received ${rate(network.receiveRate)} · Sent ${rate(network.transmitRate)}` : 'Not measured yet'}
            icon="network"
            icon-color="blue"
            data-testid="processes-network-{network.name}"
          ></ui-row>
        {:else}
          <ui-row heading="No Network Interfaces" subheading="This machine did not report any."></ui-row>
        {/each}
      </ui-section>
    </ui-list>

    <ui-list variant="insetGrouped" label="This Machine">
      <ui-section header="This Machine">
        {#if machine.name}<ui-row heading="Name" value={machine.name}></ui-row>{/if}
        {#if os !== ''}<ui-row heading="System" value={os}></ui-row>{/if}
        <ui-row heading="Architecture" value={machine.arch}></ui-row>
        <ui-row heading="Up For" value={duration(machine.uptimeSecs)} data-testid="processes-uptime"></ui-row>
        <ui-row heading="Processes" value={String(overview.processCount)}></ui-row>
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .overview {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-grouped-elevated);
    overflow: auto;
  }
  .bar {
    flex: none;
    padding: 4px 16px 4px 28px;
  }
  .bar h1 {
    margin: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(300px, 1fr));
    gap: 4px 0;
    padding: 0 12px 16px;
    align-items: start;
  }
  .stack {
    flex: 1;
    min-width: 0;
    padding: 12px 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .line {
    display: flex;
    justify-content: space-between;
    align-items: baseline;
    gap: 12px;
    font-variant-numeric: tabular-nums;
  }
  .cores {
    display: flex;
    gap: 8px;
  }
  .core {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    font-variant-numeric: tabular-nums;
  }
</style>
