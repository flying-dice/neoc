<!-- @organism ServerHeader — the web server at the top of the window: how it is doing, and, when it has sites to serve and is not serving them, why and the button that tries again -->
<script lang="ts">
  type Server = VosApp.Pages.Server
  type Site = VosApp.Pages.Site
  import { StateDot } from '@vos/docker'
  import { attentionOf, serverLabel, serverTone } from '../lib/format'

  interface Props {
    server: Server
    sites: readonly Site[]
    /** The container is being made, started or removed. */
    working?: boolean
    /** Make the container again and start it. */
    onapply: () => void
  }

  let { server, sites, working = false, onapply }: Props = $props()

  const label = $derived(serverLabel(server, sites))
  // while it is being made there is nothing to say of how the last try went
  const attention = $derived(working ? null : attentionOf(server, sites))
</script>

<header class="server">
  <div class="bar">
    <StateDot tone={working ? 'busy' : serverTone(server, sites)} {label} />
    <div class="heading">
      <h1>Web Server</h1>
      <p class="state" data-testid="pages-state">{working ? 'Working' : label}</p>
    </div>
    {#if working}
      <ui-spinner label="Working on the server" data-testid="pages-working"></ui-spinner>
    {/if}
  </div>
  {#if attention}
    <ui-banner tone="warning" heading={attention.heading} data-testid="pages-attention">
      {attention.message}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button slot="action" variant="plain" size="small" onclick={onapply} data-testid="pages-apply">Try Again</ui-button>
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
