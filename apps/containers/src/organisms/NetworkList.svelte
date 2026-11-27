<!-- @organism NetworkList — the networks Docker has, each with its driver and scope, its subnet and gateway and the containers on it, the button that removes one (none for Docker's own), and the button that makes one -->
<script lang="ts">
  type Network = VosApp.Containers.Network
  import ResourceRow from '../molecules/ResourceRow.svelte'
  import SectionBar from '../molecules/SectionBar.svelte'
  import { count, listed } from '../lib/format'

  interface Props {
    networks: readonly Network[]
    /** Show the way back to the sidebar (a window too narrow for both). */
    back?: boolean
    onback?: () => void
    onremove: (network: Network) => void
    /** Make a network: the form. */
    onnew: () => void
  }

  let { networks, back = false, onback, onremove, onnew }: Props = $props()

  /** Its addresses, as its IPAM has them. */
  const addresses = (network: Network): string =>
    network.subnet === '' ? 'no subnet' : network.gateway === '' ? network.subnet : `${network.subnet} · gateway ${network.gateway}`
  /** What it is, beside its driver and scope. */
  const kind = (network: Network): string =>
    [network.driver, network.scope, ...(network.internal ? ['internal'] : []), ...(network.attachable ? ['attachable'] : []), ...(network.predefined ? ["Docker's own"] : [])].join(' · ')
</script>

<section class="list" aria-label="Networks">
  <SectionBar title="Networks" summary={networks.length === 0 ? 'No networks' : count(networks.length, 'network')} {back} {onback}>
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" square hit label="New network" onclick={onnew} data-testid="containers-networks-new">
      <ui-icon slot="icon" name="plus"></ui-icon>
    </ui-button>
  </SectionBar>
  <div class="scroll">
    {#if networks.length > 0}
      <ul class="items" data-testid="containers-networks">
        {#each networks as network (network.name)}
          <ResourceRow
            name={network.name}
            lines={[kind(network), addresses(network)]}
            note={network.containers.length === 0 ? 'No containers' : `Containers: ${listed(network.containers)}`}
            unused={network.containers.length === 0}
            testid="containers-network-{network.name}"
            actions={network.predefined
              ? []
              : [{ label: `Remove ${network.name}`, icon: 'trash', destructive: true, testid: `containers-network-${network.name}-remove`, onclick: () => onremove(network) }]}
          />
        {/each}
      </ul>
    {:else}
      <ui-empty-state icon="network" heading="No Networks" message="Docker lists none, not even its own." data-testid="containers-networks-empty"></ui-empty-state>
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
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 12px 12px;
  }
  .items {
    margin: 0;
    padding: 0;
  }
</style>
