<!-- @organism VolumeList — the volumes Docker has, each with its driver, where its data is, the day it was made and the containers that use it, the button that removes one, the button that makes one, and the prune of those no container uses, which deletes their data, with what it freed -->
<script lang="ts">
  type Volume = VosApp.Containers.Volume
  import ResourceRow from '../molecules/ResourceRow.svelte'
  import SectionBar from '../molecules/SectionBar.svelte'
  import { count, day, listed } from '../lib/format'
  import { unusedOf } from '../lib/resources.svelte'

  interface Props {
    volumes: readonly Volume[]
    /** The unused volumes are being pruned. */
    pruning?: boolean
    /** What the last prune took, in words. */
    report?: string
    /** Show the way back to the sidebar (a window too narrow for both). */
    back?: boolean
    onback?: () => void
    onremove: (volume: Volume) => void
    /** Make a volume: the form. */
    onnew: () => void
    onprune: () => void
  }

  let { volumes, pruning = false, report, back = false, onback, onremove, onnew, onprune }: Props = $props()

  const unused = $derived(unusedOf(volumes))
  // a prune deletes data: the person is asked first, and told which volumes go
  let asking = $state(false)
</script>

<section class="list" aria-label="Volumes">
  <SectionBar
    title="Volumes"
    summary={volumes.length === 0 ? 'No volumes' : `${count(volumes.length, 'volume')} · ${unused.length} unused`}
    {back}
    {onback}
  >
    {#if pruning}<ui-spinner label="Deleting the unused volumes" data-testid="containers-volumes-pruning"></ui-spinner>{/if}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant="plain"
      square
      hit
      destructive
      disabled={pruning || unused.length === 0}
      label="Delete the unused volumes"
      onclick={() => (asking = true)}
      data-testid="containers-volumes-prune"
    >
      <ui-icon slot="icon" name="trash"></ui-icon>
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" square hit label="New volume" onclick={onnew} data-testid="containers-volumes-new">
      <ui-icon slot="icon" name="plus"></ui-icon>
    </ui-button>
  </SectionBar>
  {#if asking}
    <ui-alert
      open
      destructive
      action-label="Delete"
      heading="Delete {count(unused.length, 'unused volume')}?"
      message="{listed(unused.map((v) => v.name))} {unused.length === 1 ? 'is' : 'are'} deleted, and all the data in {unused.length === 1 ? 'it' : 'them'}. This cannot be undone."
      data-testid="containers-volumes-prune-confirm"
      oncancel={() => (asking = false)}
      onconfirm={() => {
        asking = false
        onprune()
      }}
    ></ui-alert>
  {/if}
  <div class="scroll">
    {#if report}
      <ui-banner tone="success" data-testid="containers-volumes-report">{report}</ui-banner>
    {/if}
    {#if volumes.length > 0}
      <ul class="items" data-testid="containers-volumes">
        {#each volumes as volume (volume.name)}
          {@const made = day(new Date(volume.created))}
          <ResourceRow
            name={volume.name}
            lines={[`${volume.driver} · ${volume.mountpoint}`, ...(made === '' ? [] : [`made ${made}`])]}
            note={volume.containers.length === 0 ? 'Unused' : `Used by ${listed(volume.containers)}`}
            unused={volume.containers.length === 0}
            testid="containers-volume-{volume.name}"
            actions={[
              { label: `Remove ${volume.name}`, icon: 'trash', destructive: true, testid: `containers-volume-${volume.name}-remove`, onclick: () => onremove(volume) }
            ]}
          />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="folder"
        heading="No Volumes"
        message="Docker has none. Make one with the + button, or give a new container one: Docker makes it."
        data-testid="containers-volumes-empty"
      ></ui-empty-state>
    {/if}
  </div>
</section>

<style>
  .list {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    display: flex;
    flex-direction: column;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 12px 12px;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .items {
    margin: 0;
    padding: 0;
  }
</style>
