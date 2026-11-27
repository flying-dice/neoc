<!-- @molecule RecordRow — one record in the list: the name, the address it answers with, whether every name under it does too, and the button that takes it away -->
<script lang="ts">
  type DnsRecord = VosApp.Dns.Record

  interface Props {
    record: DnsRecord
    /** A call is in flight for it: the button gives way to a spinner. */
    busy?: boolean
    /** Nothing can be removed just now (another change is in flight). */
    disabled?: boolean
    onremove: () => void
  }

  let { record, busy = false, disabled = false, onremove }: Props = $props()

  const id = $derived(`dns-record-${record.id}`)
</script>

<li class="row" data-testid={id}>
  <span class="text">
    <span class="name" data-testid="{id}-name">{record.subdomains ? `${record.name} and *.${record.name}` : record.name}</span>
    <span class="address" data-testid="{id}-address">{record.address}</span>
  </span>
  <span class="act">
    {#if busy}
      <ui-spinner label="Removing {record.name}" data-testid="{id}-busy"></ui-spinner>
    {:else}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="plain"
        square
        destructive
        hit
        {disabled}
        label="Remove {record.name} at {record.address}"
        onclick={onremove}
        data-testid="{id}-remove"
      >
        <ui-icon slot="icon" name="trash"></ui-icon>
      </ui-button>
    {/if}
  </span>
</li>

<style>
  .row {
    list-style: none;
    display: flex;
    align-items: center;
    gap: 8px;
    padding-inline-start: 12px;
    border-bottom: 1px solid var(--separator);
  }
  .row:last-child {
    border-bottom: 0;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 6px 0;
  }
  .name,
  .address {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font: var(--type-body);
    font-weight: 600;
  }
  .address {
    font: var(--type-footnote);
    font-family: var(--font-mono);
    color: var(--label-secondary);
  }
  /* the button and the spinner that stands in for it take the same room */
  .act {
    flex: none;
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
  }
</style>
