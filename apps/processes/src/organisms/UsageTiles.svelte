<!-- @organism UsageTiles — the top of the list: how busy the processor is, how much memory and swap is in use, and what the network is carrying -->
<script lang="ts">
  import type { Overview } from '@processes/rpc'
  import UsageTile from '../molecules/UsageTile.svelte'
  import { bytes, rate, share, whole } from '../lib/format'

  interface Props {
    overview: Overview
  }

  let { overview }: Props = $props()

  const cores = $derived(overview.cpu.logicalCores === 1 ? '1 core' : `${overview.cpu.logicalCores} cores`)
  // all interfaces together: a rate that is not there yet (the first reading) is not 0
  const rates = $derived(overview.networks.flatMap((network) => (network.receiveRate === undefined ? [] : [network])))
  const received = $derived(rates.length === 0 ? undefined : rates.reduce((sum, n) => sum + (n.receiveRate ?? 0), 0))
  const sent = $derived(rates.length === 0 ? undefined : rates.reduce((sum, n) => sum + (n.transmitRate ?? 0), 0))
</script>

<div class="tiles" data-testid="processes-tiles">
  <UsageTile heading="CPU" value={whole(overview.cpu.usage)} note={cores} share={overview.cpu.usage} testid="processes-tile-cpu" />
  <UsageTile
    heading="Memory"
    value={bytes(overview.memory.used)}
    note="of {bytes(overview.memory.total)}"
    share={share(overview.memory.used, overview.memory.total)}
    testid="processes-tile-memory"
  />
  <UsageTile
    heading="Swap"
    value={bytes(overview.swap.used)}
    note={overview.swap.total > 0 ? `of ${bytes(overview.swap.total)}` : 'none'}
    share={share(overview.swap.used, overview.swap.total)}
    testid="processes-tile-swap"
  />
  <ui-tile heading="Network" data-testid="processes-tile-network">
    <span class="rates">
      <span class="rate" data-testid="processes-tile-network-received">
        <ui-icon name="arrow.up" size={14} weight={2.5} style="transform: rotate(180deg)"></ui-icon>
        <span class="ios-headline">{rate(received)}</span>
      </span>
      <span class="rate" data-testid="processes-tile-network-sent">
        <ui-icon name="arrow.up" size={14} weight={2.5}></ui-icon>
        <span class="ios-headline">{rate(sent)}</span>
      </span>
    </span>
    <span slot="footer" class="ios-footnote ios-secondary">Received and sent, all interfaces.</span>
  </ui-tile>
</div>

<style>
  .tiles {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: 10px;
    padding: 4px 16px 12px;
  }
  .rates {
    display: inline-flex;
    flex-wrap: wrap;
    gap: 4px 10px;
  }
  .rate {
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }
</style>
