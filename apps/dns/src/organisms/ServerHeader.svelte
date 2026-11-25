<!-- @organism ServerHeader — the server at the top of the window: how it is doing, the switch that turns it on and off, and, when it is wanted on and is not answering, why and the button that tries again -->
<script lang="ts">
  import type { Server, Settings } from '@dns/rpc'
  import { StateDot } from '@vos/docker'
  import { attentionOf, serverLabel, serverTone } from '../lib/format'

  interface Props {
    server: Server
    settings: Settings
    /** The container is being made, started or removed. */
    working?: boolean
    onenable: (enabled: boolean) => void
    /** Make the container again and start it. */
    onapply: () => void
  }

  let { server, settings, working = false, onenable, onapply }: Props = $props()

  // The switch flips itself when it is tapped. When the change did not happen (the call failed), what it shows is put
  // back to what is so, once the call is over.
  let toggle = $state<HTMLElement & { checked: boolean }>()
  $effect(() => {
    if (toggle && !working) toggle.checked = server.enabled
  })

  const label = $derived(serverLabel(server, settings))
  // while it is being made there is nothing to say of how the last try went
  const attention = $derived(working ? null : attentionOf(server))
</script>

<header class="server">
  <div class="bar">
    <StateDot tone={working ? 'busy' : serverTone(server)} {label} />
    <div class="heading">
      <h1>DNS Server</h1>
      <p class="state" data-testid="dns-state">{working ? 'Working' : label}</p>
    </div>
    {#if working}
      <ui-spinner label="Working on the server" data-testid="dns-working"></ui-spinner>
    {/if}
    <ui-toggle
      bind:this={toggle}
      hit
      label="DNS server"
      checked={server.enabled}
      disabled={working}
      onchange={(e) => onenable(e.detail.checked)}
      data-testid="dns-enabled"
    ></ui-toggle>
  </div>
  {#if attention}
    <ui-banner tone="warning" heading={attention.heading} data-testid="dns-attention">
      {attention.message}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button slot="action" variant="plain" size="small" onclick={onapply} data-testid="dns-apply">Try Again</ui-button>
    </ui-banner>
  {/if}
</header>

<style>
  .server {
    display: flex;
    flex-direction: column;
    gap: 8px;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 16px 8px;
  }
  .bar {
    display: flex;
    align-items: center;
    gap: 10px;
    min-height: 44px;
  }
  .heading {
    flex: 1;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font: var(--type-title3);
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .state {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
</style>
