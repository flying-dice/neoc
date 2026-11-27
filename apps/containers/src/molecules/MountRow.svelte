<!-- @molecule MountRow — one mount of the form: a folder of the machine or a volume (one that is there, or a new one named here), the place in the container it is at, whether it is read-only, and the button that takes it out -->
<script lang="ts">
  type MountKind = VosApp.Containers.MountKind
  import type { DraftMount } from '../lib/spec'

  interface Props {
    /** The row, changed as it is typed in. */
    mount: DraftMount
    /** The names of the volumes there are, to pick from. */
    volumes: readonly string[]
    disabled?: boolean
    /** The row's test id; its fields' and button's follow from it. */
    testid: string
    onremove: () => void
  }

  let { mount = $bindable(), volumes, disabled = false, testid, onremove }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const KINDS = [
    { value: 'bind', label: 'Folder' },
    { value: 'volume', label: 'Volume' }
  ]
  // the picker's choice that names a new volume: no volume's name starts with +
  const NEW = '+new'
  const choices = $derived([...volumes.map((name) => ({ value: name, label: name })), { value: NEW, label: 'New volume…' }])

  // a volume starts as the first one there is, or a new one when there is none
  function kind(next: MountKind): void {
    mount.kind = next
    mount.fresh = next === 'bind' || volumes.length === 0
    mount.source = mount.fresh ? '' : (volumes[0] ?? '')
  }
  function pick(value: string): void {
    mount.fresh = value === NEW
    mount.source = value === NEW ? '' : value
  }
</script>

<ui-row data-testid={testid}>
  <div class="mount">
    <span class="line">
      <ui-segmented
        label="What is mounted"
        options={KINDS}
        value={mount.kind}
        {disabled}
        onchange={(e) => kind(e.detail.value as MountKind)}
        data-testid="{testid}-kind"
      ></ui-segmented>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant={mount.read_only ? 'tinted' : 'gray'}
        size="small"
        toggle
        pressed={mount.read_only}
        hit
        {disabled}
        onclick={() => (mount.read_only = !mount.read_only)}
        data-testid="{testid}-read-only"
      >
        Read-only
      </ui-button>
    </span>
    <span class="line">
      {#if mount.kind === 'bind'}
        <ui-text-field
          class="source"
          label="The folder on the machine"
          placeholder="/srv/www"
          autocapitalize="off"
          autocomplete="off"
          variant="field"
          {disabled}
          value={mount.source}
          oninput={(e: Event) => (mount.source = valueOf(e))}
          data-testid="{testid}-source"
        ></ui-text-field>
      {:else}
        <ui-picker
          class="source"
          label="The volume"
          options={choices}
          value={mount.fresh ? NEW : mount.source}
          variant="field"
          {disabled}
          onchange={(e) => pick(e.detail.value)}
          data-testid="{testid}-volume"
        ></ui-picker>
        {#if mount.fresh}
          <ui-text-field
            class="source"
            label="The new volume's name"
            placeholder="Name, or empty"
            autocapitalize="off"
            autocomplete="off"
            variant="field"
            {disabled}
            value={mount.source}
            oninput={(e: Event) => (mount.source = valueOf(e))}
            data-testid="{testid}-source"
          ></ui-text-field>
        {/if}
      {/if}
      <ui-icon class="to" name="chevron.right" size={12} label="at"></ui-icon>
      <ui-text-field
        class="target"
        label="The place in the container"
        placeholder="/data"
        autocapitalize="off"
        autocomplete="off"
        variant="field"
        {disabled}
        value={mount.target}
        oninput={(e: Event) => (mount.target = valueOf(e))}
        data-testid="{testid}-target"
      ></ui-text-field>
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
    label="Remove this mount"
    onclick={onremove}
    data-testid="{testid}-remove"
  >
    <ui-icon slot="icon" name="minus"></ui-icon>
  </ui-button>
</ui-row>

<style>
  .mount {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 8px;
    padding-block: 2px;
  }
  .line {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 6px 8px;
  }
  .source,
  .target {
    flex: 1 1 9em;
    min-width: 0;
  }
  .to {
    flex: none;
    color: var(--label-tertiary);
  }
</style>
