<!-- @molecule SiteRow — one site in the list: its name, its folder, each address it is reached at as a button that opens it, the switch that turns it on and off, and the buttons that change it and take it away -->
<script lang="ts">
  type Site = VosApp.Pages.Site
  import type { Address } from '../lib/format'

  interface Props {
    site: Site
    /** Every way the site is reached. */
    addresses: readonly Address[]
    /** A call is in flight for it: the buttons give way to a spinner. */
    busy?: boolean
    /** Nothing can be changed just now (another change is in flight). */
    disabled?: boolean
    onenable: (enabled: boolean) => void
    onopen: (url: string) => void
    onedit: () => void
    onremove: () => void
  }

  let { site, addresses, busy = false, disabled = false, onenable, onopen, onedit, onremove }: Props = $props()

  const id = $derived(`pages-site-${site.id}`)

  // The switch flips itself when it is tapped. When the change did not happen (the call failed), what it shows is put
  // back to what is so, once the call is over.
  let toggle = $state<HTMLElement & { checked: boolean }>()
  $effect(() => {
    if (toggle && !busy && !disabled) toggle.checked = site.enabled
  })
</script>

<ui-row class:off={!site.enabled} data-testid={id}>
  <div class="body">
    <span class="text">
      <span class="name" data-testid="{id}-name">{site.name}</span>
      <span class="folder" data-testid="{id}-folder">{site.folder}</span>
    </span>
    {#if site.problem}
      <p class="problem" data-testid="{id}-problem">Not served: {site.problem}</p>
    {/if}
    <div class="addresses">
      {#each addresses as address, at (address.url)}
        {#if site.enabled && !site.problem}
          <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
          <ui-button
            variant="tinted"
            size="small"
            shape="capsule"
            hit
            label="Open {address.label}"
            onclick={() => onopen(address.url)}
            data-testid="{id}-open-{at}"
          >
            {address.label}
          </ui-button>
        {:else}
          <span class="address" data-testid="{id}-address-{at}">{address.label}</span>
        {/if}
      {/each}
    </div>
  </div>
  <span class="trailing" slot="trailing">
    {#if busy}
      <span class="act"><ui-spinner label="Working on {site.name}" data-testid="{id}-busy"></ui-spinner></span>
    {:else}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        square
        hit
        {disabled}
        title="Change {site.name}"
        label="Change {site.name}"
        onclick={onedit}
        data-testid="{id}-edit"
      >
        <ui-icon slot="icon" name="square.and.pencil"></ui-icon>
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        square
        destructive
        hit
        {disabled}
        title="Remove {site.name}"
        label="Remove {site.name}"
        onclick={onremove}
        data-testid="{id}-remove"
      >
        <ui-icon slot="icon" name="trash"></ui-icon>
      </ui-button>
    {/if}
    <ui-toggle
      bind:this={toggle}
      hit
      label="Serve {site.name}"
      checked={site.enabled}
      disabled={busy || disabled}
      onchange={(e) => onenable(e.detail.checked)}
      data-testid="{id}-enabled"
    ></ui-toggle>
  </span>
</ui-row>

<style>
  .body {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 8px 0;
  }
  .trailing {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .text {
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name,
  .folder {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font: var(--type-body);
    font-weight: 600;
  }
  ui-row.off .name {
    color: var(--label-secondary);
  }
  .folder {
    font: var(--type-footnote);
    font-family: var(--font-mono);
    color: var(--label-secondary);
  }
  /* the spinner takes the room of the two buttons it stands in for */
  .act {
    flex: none;
    width: 88px;
    height: 44px;
    display: grid;
    place-items: center;
  }
  ui-toggle {
    margin-inline-start: 8px;
  }
  .problem {
    margin: 0;
    font: var(--type-footnote);
    color: var(--ios-red);
  }
  .addresses {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .address {
    font: var(--type-footnote);
    font-family: var(--font-mono);
    color: var(--label-secondary);
    overflow-wrap: anywhere;
  }
</style>
