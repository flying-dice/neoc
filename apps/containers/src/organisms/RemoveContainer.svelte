<!-- @organism RemoveContainer — the question before a container is removed: what goes with it (a running one only when forced, its anonymous volumes when asked), that Compose makes a stack's container again, and why Docker refused -->
<script lang="ts">
  import type { Removal } from '../lib/containers.svelte'
  import { imageLabel, isUp } from '../lib/format'

  interface Props {
    removal: Removal
    onremove: () => void
    oncancel: () => void
  }

  let { removal = $bindable(), onremove, oncancel }: Props = $props()

  const container = $derived(removal.container)
  const up = $derived(isUp(container))
  // Docker removes a running container only when forced: the button waits for that choice
  const ready = $derived(!removal.busy && (!up || removal.force))
</script>

<section class="remove" aria-label="Remove {container.name}" data-testid="containers-remove">
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" hit disabled={removal.busy} onclick={oncancel} data-testid="containers-remove-cancel">Cancel</ui-button>
    <h1>Remove {container.name}?</h1>
    <span class="end">
      {#if removal.busy}<ui-spinner label="Removing {container.name}" data-testid="containers-remove-busy"></ui-spinner>{/if}
    </span>
  </header>
  <div class="fields">
    <ui-list variant="inset" label="What goes with it">
      <ui-section
        header="Container"
        footer={container.stack
          ? `It is the ${container.service} service of the Compose stack ${container.stack}. Compose makes it again on the next docker compose up.`
          : 'Removing it cannot be undone. Its named volumes stay.'}
        data-testid="containers-remove-about"
      >
        <ui-row heading={container.name} subheading={imageLabel(container.image)} value={up ? 'Running' : 'Stopped'}></ui-row>
      </ui-section>
      {#if up}
        <ui-section footer="Docker removes a running container only when it is forced, and kills it first.">
          <ui-row heading="Remove it while it runs">
            <ui-toggle
              slot="trailing"
              label="Remove it while it runs"
              checked={removal.force}
              disabled={removal.busy}
              onchange={(e) => (removal.force = e.detail.checked)}
              data-testid="containers-remove-force"
            ></ui-toggle>
          </ui-row>
        </ui-section>
      {/if}
      <ui-section footer="The volumes Docker made for it alone, without a name. Named volumes always stay.">
        <ui-row heading="Remove its anonymous volumes">
          <ui-toggle
            slot="trailing"
            label="Remove its anonymous volumes"
            checked={removal.volumes}
            disabled={removal.busy}
            onchange={(e) => (removal.volumes = e.detail.checked)}
            data-testid="containers-remove-volumes"
          ></ui-toggle>
        </ui-row>
      </ui-section>
      <ui-section footer={removal.failure ?? undefined} footer-tone="error" data-testid="containers-remove-failure">
        <ui-row
          heading="Remove {container.name}"
          destructive
          tappable
          disabled={!ready}
          onpress={onremove}
          data-testid="containers-remove-confirm"
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
