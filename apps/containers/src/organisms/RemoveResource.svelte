<!-- @organism RemoveResource — the question before an image, a volume or a network is removed: what it is, which containers use it and what Docker does about them (an image only when forced, a volume not while one uses it), and why it was refused -->
<script lang="ts">
  import type { Dropping } from '../lib/resources.svelte'
  import { listed } from '../lib/format'

  interface Props {
    removal: Dropping
    onremove: () => void
    oncancel: () => void
  }

  let { removal = $bindable(), onremove, oncancel }: Props = $props()

  const used = $derived(removal.users.length > 0)
  const they = $derived(listed(removal.users))
  // what goes with it, and what Docker does about the containers that use it
  const about = $derived.by((): string => {
    if (removal.kind === 'image') {
      return used
        ? `Made from it: ${they}. Docker removes an image a container was made from only when it is forced, and never one a running container was made from.`
        : 'Its names and its layers go. Docker pulls it again when a container needs it.'
    }
    if (removal.kind === 'volume') {
      return used
        ? `${they} ${removal.users.length === 1 ? 'uses' : 'use'} it: Docker removes a volume only once no container uses it, running or not.`
        : 'The data in it is deleted with it. This cannot be undone.'
    }
    return used
      ? `On it: ${they}. Docker removes a network only once no running container is on it.`
      : 'No container is on it. Removing it cannot be undone.'
  })
  // an image a container was made from waits for force; a volume a container uses cannot be removed at all
  const blocked = $derived(removal.kind === 'volume' && used)
  const ready = $derived(!removal.busy && !blocked && (removal.kind !== 'image' || !used || removal.force))
  const noun = $derived(removal.kind === 'image' ? 'Image' : removal.kind === 'volume' ? 'Volume' : 'Network')
</script>

<section class="remove" aria-label="Remove {removal.name}" data-testid="containers-remove-resource">
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={removal.busy} onclick={oncancel} data-testid="containers-remove-resource-cancel">Cancel</ui-button>
    <h1>Remove {removal.name}?</h1>
    <span class="end">
      {#if removal.busy}<ui-spinner label="Removing {removal.name}" data-testid="containers-remove-resource-busy"></ui-spinner>{/if}
    </span>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="What goes">
      <ui-section header={noun} footer={about} data-testid="containers-remove-resource-about">
        <ui-row heading={removal.name} subheading={removal.about} value={used ? `${removal.users.length} using it` : 'Unused'}></ui-row>
      </ui-section>
      {#if removal.kind === 'image' && used}
        <ui-section footer="Its names go, and the image with them; the containers made from it stay, by its id.">
          <ui-row heading="Remove it though containers were made from it">
            <ui-toggle
              slot="trailing"
              label="Remove it though containers were made from it"
              checked={removal.force}
              disabled={removal.busy}
              onchange={(e) => (removal.force = e.detail.checked)}
              data-testid="containers-remove-resource-force"
            ></ui-toggle>
          </ui-row>
        </ui-section>
      {/if}
      <ui-section footer={removal.failure ?? undefined} footer-tone="error" data-testid="containers-remove-resource-failure">
        <ui-row
          heading={removal.kind === 'volume' ? `Delete ${removal.name} and its data` : `Remove ${removal.name}`}
          action
          destructive
          tappable
          disabled={!ready}
          onpress={onremove}
          data-testid="containers-remove-resource-confirm"
        ></ui-row>
      </ui-section>
    </ui-list>
  </div>
</section>

<style>
  .remove {
    box-sizing: border-box;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
    background: var(--bg-secondary);
  }
  .bar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 8px;
    padding: 0 8px 6px;
    border-bottom: 1px solid var(--separator);
  }
  .bar > :first-child {
    justify-self: start;
  }
  .end {
    justify-self: end;
    display: flex;
  }
  h1 {
    min-width: 0;
    margin: 0;
    font: var(--type-headline);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .fields {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 16px 12px;
  }
</style>
