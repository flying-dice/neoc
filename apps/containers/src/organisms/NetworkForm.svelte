<!-- @organism NetworkForm — the form a network is made in: its name and driver, its subnet and gateway, whether it is internal or attachable, its labels, and why the backend refused it -->
<script lang="ts">
  import EntryRow from '../molecules/EntryRow.svelte'
  import type { NetworkDraft, ObjectForm } from '../lib/resources.svelte'
  import { blankEntry } from '../lib/spec'

  interface Props {
    form: ObjectForm<NetworkDraft>
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  let { form = $bindable(), savable, onsave, oncancel }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const saving = $derived(form.saving)
  const DRIVERS = [
    { value: 'bridge', label: 'bridge' },
    { value: 'overlay', label: 'overlay' },
    { value: 'macvlan', label: 'macvlan' },
    { value: 'ipvlan', label: 'ipvlan' }
  ]
</script>

<form
  class="form"
  aria-label="New Network"
  data-testid="containers-network-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={saving} onclick={oncancel} data-testid="containers-network-form-cancel">Cancel</ui-button>
    <h1 data-testid="containers-network-form-title">New Network</h1>
    <span class="save">
      {#if saving}<ui-spinner label="Saving" data-testid="containers-network-form-saving"></ui-spinner>{/if}
      <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="containers-network-form-save">Create</ui-button>
    </span>
  </header>
  <div class="fields">
    {#if form.failure}
      <ui-banner tone="error" data-testid="containers-network-form-failure">{form.failure}</ui-banner>
    {/if}
    <ui-list variant="inset" label="The network">
      <ui-section header="Network" footer="bridge joins containers on this machine; overlay, those of a swarm; macvlan and ipvlan put them on the machine's own network.">
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            label="Name"
            placeholder="backend"
            autocapitalize="off"
            autocomplete="off"
            maxlength={128}
            variant="bare"
            disabled={saving}
            value={form.draft.name}
            oninput={(e: Event) => (form.draft.name = valueOf(e))}
            data-testid="containers-network-form-name"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Driver</span>
          <ui-picker
            label="Driver"
            options={DRIVERS}
            value={form.draft.driver}
            variant="bare"
            disabled={saving}
            onchange={(e) => (form.draft.driver = e.detail.value)}
            data-testid="containers-network-form-driver"
          ></ui-picker>
        </ui-row>
      </ui-section>
      <ui-section header="Addresses" footer="Empty for addresses Docker chooses. A gateway is an address in the subnet.">
        <ui-row>
          <span class="key">Subnet</span>
          <ui-text-field
            class="mono"
            label="Subnet"
            placeholder="172.30.0.0/16"
            autocapitalize="off"
            autocomplete="off"
            maxlength={64}
            variant="bare"
            disabled={saving}
            value={form.draft.subnet}
            oninput={(e: Event) => (form.draft.subnet = valueOf(e))}
            data-testid="containers-network-form-subnet"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Gateway</span>
          <ui-text-field
            class="mono"
            label="Gateway"
            placeholder="172.30.0.1"
            autocapitalize="off"
            autocomplete="off"
            maxlength={64}
            variant="bare"
            disabled={saving}
            value={form.draft.gateway}
            oninput={(e: Event) => (form.draft.gateway = valueOf(e))}
            data-testid="containers-network-form-gateway"
          ></ui-text-field>
        </ui-row>
      </ui-section>
      <ui-section footer="An internal network's containers reach each other, and not the world. An attachable one can be joined by a container started by hand, as a swarm's can.">
        <ui-row heading="Internal">
          <ui-toggle
            slot="trailing"
            label="Internal"
            checked={form.draft.internal}
            disabled={saving}
            onchange={(e) => (form.draft.internal = e.detail.checked)}
            data-testid="containers-network-form-internal"
          ></ui-toggle>
        </ui-row>
        <ui-row heading="Attachable">
          <ui-toggle
            slot="trailing"
            label="Attachable"
            checked={form.draft.attachable}
            disabled={saving}
            onchange={(e) => (form.draft.attachable = e.detail.checked)}
            data-testid="containers-network-form-attachable"
          ></ui-toggle>
        </ui-row>
      </ui-section>
      <ui-section header="Labels" data-testid="containers-network-form-labels">
        {#each form.draft.labels as entry, i (entry.key)}
          <EntryRow
            bind:entry={form.draft.labels[i]!}
            kind="Label"
            placeholder="com.example.key"
            disabled={saving}
            testid="containers-network-form-label-{i}"
            onremove={() => (form.draft.labels = form.draft.labels.filter((e) => e.key !== entry.key))}
          />
        {/each}
        <ui-row
          heading="Add Label"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.labels.push(blankEntry())}
          data-testid="containers-network-form-add-label"
        ></ui-row>
      </ui-section>
    </ui-list>
  </div>
</form>

<style>
  .form {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 8px 6px;
    border-bottom: 1px solid var(--separator);
  }
  h1 {
    min-width: 0;
    margin: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .save {
    display: flex;
    align-items: center;
    gap: 4px;
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
    display: flex;
    flex-direction: column;
    gap: 12px;
  }
  .mono {
    font-family: var(--font-mono);
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 5em;
  }
  ui-text-field,
  ui-picker {
    flex: 1;
    min-width: 0;
  }
</style>
