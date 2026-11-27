<!-- @organism ContainerForm — the form a container is made, changed or copied in: its image and name, what it runs, its environment, ports, mounts, network, restart policy, labels and the rest, how the pull of its image goes while it is saved, and why the backend refused it -->
<script lang="ts">
  type Network = VosApp.Containers.Network
  type Restart = VosApp.Containers.Restart
  type Volume = VosApp.Containers.Volume
  import EntryRow from '../molecules/EntryRow.svelte'
  import MountRow from '../molecules/MountRow.svelte'
  import PortRow from '../molecules/PortRow.svelte'
  import PullMeter from '../molecules/PullMeter.svelte'
  import type { ContainerForm } from '../lib/containers.svelte'
  import { blankEntry, blankMount, blankPort } from '../lib/spec'

  interface Props {
    form: ContainerForm
    networks: readonly Network[]
    volumes: readonly Volume[]
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  let { form = $bindable(), networks, volumes, savable, onsave, oncancel }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const draft = $derived(form.draft)
  const saving = $derived(form.saving)
  const named = $derived(form.of?.name ?? '')
  const heading = $derived(form.mode === 'new' ? 'New Container' : form.mode === 'edit' ? `Edit ${named}` : `Duplicate ${named}`)
  const RESTARTS = [
    { value: 'no', label: 'Never' },
    { value: 'always', label: 'Always' },
    { value: 'unless-stopped', label: 'Unless stopped' },
    { value: 'on-failure', label: 'On failure' }
  ]
  // the network the container is on stays a choice, though Docker no longer lists it
  const networkChoices = $derived.by(() => {
    const names = networks.map((n) => n.name)
    if (!names.includes(draft.network)) names.unshift(draft.network)
    return names.map((name) => ({ value: name, label: name }))
  })
  const volumeNames = $derived(volumes.map((v) => v.name))
  const pulled = $derived(form.pulling)

  // a refusal is said at the top of the form: it is scrolled to, wherever the person was
  let fields = $state<HTMLElement>()
  $effect(() => {
    if (form.failure) fields?.scrollTo({ top: 0, behavior: 'smooth' })
  })
</script>

<form
  class="form"
  aria-label={heading}
  data-testid="containers-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={saving} onclick={oncancel} data-testid="containers-form-cancel">Cancel</ui-button>
    <h1 data-testid="containers-form-title">{heading}</h1>
    <span class="save">
      {#if saving}<ui-spinner label="Saving" data-testid="containers-form-saving"></ui-spinner>{/if}
      <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="containers-form-save">
        {form.mode === 'edit' ? 'Save' : 'Create'}
      </ui-button>
    </span>
  </header>
  <!-- while it saves every control is disabled: the keyboard reaches the fields themselves, to scroll them -->
  <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
  <div class="fields" bind:this={fields} tabindex={saving ? 0 : undefined} aria-label={saving ? 'The settings being saved' : undefined}>
    {#if form.failure}
      <ui-banner tone="error" data-testid="containers-form-failure">{form.failure}</ui-banner>
    {/if}
    {#if saving && pulled}
      <PullMeter pulling={pulled} testid="containers-form-pulling" />
    {/if}
    {#if form.mode === 'edit'}
      <ui-banner tone="warning" data-testid="containers-form-recreate">
        Docker cannot change a container that is made: saving makes it again, by these settings. Anything it keeps outside a volume is lost.
      </ui-banner>
    {/if}
    <ui-list variant="inset" label="The container">
      <ui-section header="Container" footer="The image is pulled first when Docker does not have it. Without a name, Docker makes one up.">
        <ui-row>
          <span class="key">Image</span>
          <ui-text-field
            label="Image"
            placeholder="nginx:1.27"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.image}
            oninput={(e: Event) => (form.draft.image = valueOf(e))}
            data-testid="containers-form-image"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Name</span>
          <ui-text-field
            label="Name"
            placeholder="Optional"
            autocapitalize="off"
            autocomplete="off"
            maxlength={128}
            variant="bare"
            disabled={saving}
            value={draft.name}
            oninput={(e: Event) => (form.draft.name = valueOf(e))}
            data-testid="containers-form-name"
          ></ui-text-field>
        </ui-row>
      </ui-section>

      <ui-section header="Command" footer="Empty for the image's own. Words are split as a shell splits them, with quotes around a word that has spaces.">
        <ui-row>
          <span class="key">Command</span>
          <ui-text-field
            class="mono"
            label="Command"
            placeholder="The image's"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.command}
            oninput={(e: Event) => (form.draft.command = valueOf(e))}
            data-testid="containers-form-command"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Entrypoint</span>
          <ui-text-field
            class="mono"
            label="Entrypoint"
            placeholder="The image's"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.entrypoint}
            oninput={(e: Event) => (form.draft.entrypoint = valueOf(e))}
            data-testid="containers-form-entrypoint"
          ></ui-text-field>
        </ui-row>
      </ui-section>

      <ui-section header="Environment" data-testid="containers-form-environment">
        {#each form.draft.environment as entry, i (entry.key)}
          <EntryRow
            bind:entry={form.draft.environment[i]!}
            kind="Variable"
            disabled={saving}
            testid="containers-form-env-{i}"
            onremove={() => (form.draft.environment = form.draft.environment.filter((e) => e.key !== entry.key))}
          />
        {/each}
        <ui-row
          heading="Add Variable"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.environment.push(blankEntry())}
          data-testid="containers-form-add-env"
        ></ui-row>
      </ui-section>

      <ui-section
        header="Ports"
        footer="The machine's port, then the container's. Leave the machine's port empty for one Docker chooses, and its address empty for all of them."
        data-testid="containers-form-ports"
      >
        {#each form.draft.ports as port, i (port.key)}
          <PortRow
            bind:port={form.draft.ports[i]!}
            disabled={saving}
            testid="containers-form-port-{i}"
            onremove={() => (form.draft.ports = form.draft.ports.filter((p) => p.key !== port.key))}
          />
        {/each}
        <ui-row
          heading="Add Port"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.ports.push(blankPort())}
          data-testid="containers-form-add-port"
        ></ui-row>
        <ui-row heading="Publish every exposed port">
          <ui-toggle
            slot="trailing"
            label="Publish every exposed port"
            checked={draft.publish_all}
            disabled={saving}
            onchange={(e) => (form.draft.publish_all = e.detail.checked)}
            data-testid="containers-form-publish-all"
          ></ui-toggle>
        </ui-row>
      </ui-section>

      <ui-section
        header="Volumes and Mounts"
        footer="A folder of the machine, by its whole path, or a volume of Docker's: one that is there, or a new one Docker makes. A new volume without a name is an anonymous one."
        data-testid="containers-form-mounts"
      >
        {#each form.draft.mounts as mount, i (mount.key)}
          <MountRow
            bind:mount={form.draft.mounts[i]!}
            volumes={volumeNames}
            disabled={saving}
            testid="containers-form-mount-{i}"
            onremove={() => (form.draft.mounts = form.draft.mounts.filter((m) => m.key !== mount.key))}
          />
        {/each}
        <ui-row
          heading="Add Folder"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.mounts.push(blankMount('bind'))}
          data-testid="containers-form-add-bind"
        ></ui-row>
        <ui-row
          heading="Add Volume"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.mounts.push({ ...blankMount('volume'), fresh: volumeNames.length === 0, source: volumeNames[0] ?? '' })}
          data-testid="containers-form-add-volume"
        ></ui-row>
      </ui-section>

      <ui-section header="Network and Restart">
        <ui-row>
          <span class="key">Network</span>
          <ui-picker
            label="Network"
            options={networkChoices}
            value={draft.network}
            variant="bare"
            disabled={saving}
            onchange={(e) => (form.draft.network = e.detail.value)}
            data-testid="containers-form-network"
          ></ui-picker>
        </ui-row>
        <ui-row>
          <span class="key">Restart</span>
          <ui-picker
            label="Restart"
            options={RESTARTS}
            value={draft.restart}
            variant="bare"
            disabled={saving}
            onchange={(e) => (form.draft.restart = e.detail.value as Restart)}
            data-testid="containers-form-restart"
          ></ui-picker>
        </ui-row>
        {#if draft.restart === 'on-failure'}
          <ui-row>
            <span class="key">Retries</span>
            <ui-text-field
              label="Retries"
              placeholder="No limit"
              inputmode="numeric"
              autocomplete="off"
              variant="bare"
              disabled={saving}
              value={draft.retries}
              oninput={(e: Event) => (form.draft.retries = valueOf(e))}
              data-testid="containers-form-retries"
            ></ui-text-field>
          </ui-row>
        {/if}
      </ui-section>

      <ui-section header="Labels" data-testid="containers-form-labels">
        {#each form.draft.labels as entry, i (entry.key)}
          <EntryRow
            bind:entry={form.draft.labels[i]!}
            kind="Label"
            placeholder="com.example.key"
            disabled={saving}
            testid="containers-form-label-{i}"
            onremove={() => (form.draft.labels = form.draft.labels.filter((e) => e.key !== entry.key))}
          />
        {/each}
        <ui-row
          heading="Add Label"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.labels.push(blankEntry())}
          data-testid="containers-form-add-label"
        ></ui-row>
      </ui-section>

      <ui-section header="Advanced" footer="Empty for the image's own, or Docker's.">
        <ui-row>
          <span class="key">Folder</span>
          <ui-text-field
            class="mono"
            label="Working folder"
            placeholder="/app"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.working_dir}
            oninput={(e: Event) => (form.draft.working_dir = valueOf(e))}
            data-testid="containers-form-working-dir"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">User</span>
          <ui-text-field
            label="User"
            placeholder="1000:1000"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.user}
            oninput={(e: Event) => (form.draft.user = valueOf(e))}
            data-testid="containers-form-user"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Host name</span>
          <ui-text-field
            label="Host name"
            placeholder="Docker's"
            autocapitalize="off"
            autocomplete="off"
            variant="bare"
            disabled={saving}
            value={draft.hostname}
            oninput={(e: Event) => (form.draft.hostname = valueOf(e))}
            data-testid="containers-form-hostname"
          ></ui-text-field>
        </ui-row>
      </ui-section>

      <ui-section>
        <ui-row heading={form.mode === 'edit' ? 'Start it once it is made again' : 'Start it now'}>
          <ui-toggle
            slot="trailing"
            label={form.mode === 'edit' ? 'Start it once it is made again' : 'Start it now'}
            checked={draft.start}
            disabled={saving}
            onchange={(e) => (form.draft.start = e.detail.checked)}
            data-testid="containers-form-start"
          ></ui-toggle>
        </ui-row>
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
  .fields:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .mono {
    font-family: var(--font-mono);
  }
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 6em;
  }
  ui-text-field,
  ui-picker {
    flex: 1;
    min-width: 0;
  }
</style>
