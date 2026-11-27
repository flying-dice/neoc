<!-- @organism VolumeForm — the form a volume is made in: its name, the driver that keeps it and its labels, and why the backend refused it -->
<script lang="ts">
  import EntryRow from '../molecules/EntryRow.svelte'
  import type { ObjectForm, VolumeDraft } from '../lib/resources.svelte'
  import { blankEntry } from '../lib/spec'

  interface Props {
    form: ObjectForm<VolumeDraft>
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  let { form = $bindable(), savable, onsave, oncancel }: Props = $props()

  const valueOf = (e: Event): string => (e.currentTarget as unknown as { value: string }).value
  const saving = $derived(form.saving)
</script>

<form
  class="form"
  aria-label="New Volume"
  data-testid="containers-volume-form"
  onsubmit={(e) => {
    e.preventDefault()
    onsave()
  }}
>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={saving} onclick={oncancel} data-testid="containers-volume-form-cancel">Cancel</ui-button>
    <h1 data-testid="containers-volume-form-title">New Volume</h1>
    <span class="save">
      {#if saving}<ui-spinner label="Saving" data-testid="containers-volume-form-saving"></ui-spinner>{/if}
      <ui-button type="submit" variant="plain" hit disabled={!savable} data-testid="containers-volume-form-save">Create</ui-button>
    </span>
  </header>
  <div class="fields">
    {#if form.failure}
      <ui-banner tone="error" data-testid="containers-volume-form-failure">{form.failure}</ui-banner>
    {/if}
    <ui-list variant="inset" label="The volume">
      <ui-section header="Volume" footer="Without a name, Docker makes one up. Docker's own driver, local, keeps the data on the machine Docker runs on.">
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
            value={form.draft.name}
            oninput={(e: Event) => (form.draft.name = valueOf(e))}
            data-testid="containers-volume-form-name"
          ></ui-text-field>
        </ui-row>
        <ui-row>
          <span class="key">Driver</span>
          <ui-text-field
            label="Driver"
            placeholder="local"
            autocapitalize="off"
            autocomplete="off"
            maxlength={128}
            variant="bare"
            disabled={saving}
            value={form.draft.driver}
            oninput={(e: Event) => (form.draft.driver = valueOf(e))}
            data-testid="containers-volume-form-driver"
          ></ui-text-field>
        </ui-row>
      </ui-section>
      <ui-section header="Labels" data-testid="containers-volume-form-labels">
        {#each form.draft.labels as entry, i (entry.key)}
          <EntryRow
            bind:entry={form.draft.labels[i]!}
            kind="Label"
            placeholder="com.example.key"
            disabled={saving}
            testid="containers-volume-form-label-{i}"
            onremove={() => (form.draft.labels = form.draft.labels.filter((e) => e.key !== entry.key))}
          />
        {/each}
        <ui-row
          heading="Add Label"
          action
          tappable
          disabled={saving}
          onpress={() => form.draft.labels.push(blankEntry())}
          data-testid="containers-volume-form-add-label"
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
  ui-row {
    --row-gap: 12px;
  }
  .key {
    flex: none;
    width: 5em;
  }
  ui-text-field {
    flex: 1;
    min-width: 0;
  }
</style>
