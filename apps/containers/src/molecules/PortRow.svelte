<!-- @molecule PortRow — one port of the container published on the machine: the machine's address and port, the container's port, TCP or UDP, and the button that takes it out -->
<script lang="ts">
  type Port = VosApp.Containers.Port
  type Protocol = VosApp.Containers.Protocol

  interface Props {
    /** The row, changed as it is typed in. */
    port: Port
    disabled?: boolean
    /** The row's test id; its fields' and button's follow from it. */
    testid: string
    onremove: () => void
  }

  let { port = $bindable(), disabled = false, testid, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const PROTOCOLS = [
    { value: 'tcp', label: 'TCP' },
    { value: 'udp', label: 'UDP' }
  ]
</script>

<ui-row data-testid={testid}>
  <!-- the fields wrap onto a second line in a narrow window: the machine's side, then the container's -->
  <div class="port">
    <span class="side">
      <ui-text-field
        class="ip"
        label="The machine's address"
        placeholder="All addresses"
        inputmode="decimal"
        autocomplete="off"
        variant="field"
        {disabled}
        value={port.host_ip}
        oninput={(e: Event) => (port.host_ip = valueOf(e))}
        data-testid="{testid}-host-ip"
      ></ui-text-field>
      <ui-text-field
        class="number"
        label="The machine's port"
        placeholder="Any"
        inputmode="numeric"
        autocomplete="off"
        variant="field"
        {disabled}
        value={port.host_port}
        oninput={(e: Event) => (port.host_port = valueOf(e))}
        data-testid="{testid}-host-port"
      ></ui-text-field>
    </span>
    <span class="side">
      <ui-icon class="to" name="chevron.right" size={12} label="to"></ui-icon>
      <ui-text-field
        class="number"
        label="The container's port"
        placeholder="80"
        inputmode="numeric"
        autocomplete="off"
        variant="field"
        {disabled}
        value={port.container_port}
        oninput={(e: Event) => (port.container_port = valueOf(e))}
        data-testid="{testid}-container-port"
      ></ui-text-field>
      <ui-segmented
        label="Protocol"
        options={PROTOCOLS}
        value={port.protocol}
        {disabled}
        onchange={(e) => (port.protocol = e.detail.value as Protocol)}
        data-testid="{testid}-protocol"
      ></ui-segmented>
    </span>
  </div>
  <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
  <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
  <ui-button
    slot="trailing"
    variant="plain"
    square
    size="small"
    destructive
    hit
    {disabled}
    label="Remove this port"
    onclick={onremove}
    data-testid="{testid}-remove"
  >
    <ui-icon slot="icon" name="minus"></ui-icon>
  </ui-button>
</ui-row>

<style>
  .port {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .side {
    flex: 1 1 12em;
    min-width: 0;
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .ip {
    flex: 1 1 7em;
    min-width: 0;
  }
  .number {
    flex: 0 0 5.5em;
    min-width: 0;
  }
  .to {
    flex: none;
    color: var(--label-tertiary);
  }
  ui-segmented {
    flex: none;
  }
</style>
