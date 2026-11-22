<!-- @organism SiteList — the sites that are hosted: each with its addresses, its switch and its buttons, and the button that adds one; or, while a site is typed, the form for it in their place -->
<script lang="ts">
  import type { Settings, Site } from 'app:rpc'
  import SiteForm from '../molecules/SiteForm.svelte'
  import SiteRow from '../molecules/SiteRow.svelte'
  import { addressesOf, sitesSummary } from '../lib/format'
  import type { Editing, SiteDraft } from '../lib/pages.svelte'

  interface Props {
    sites: readonly Site[]
    settings: Settings
    /** The name the machine the sites are served from is reached by. */
    machine: string
    /** The site that is being typed; nothing while the list is shown. */
    editing: Editing | null
    /** A change is in flight: the server may be being made again. */
    working?: boolean
    /** Whether a call is in flight for a site. */
    busy: (site: Site) => boolean
    onadd: () => void
    onedit: (site: Site) => void
    ondraft: (change: Partial<SiteDraft>) => void
    onsave: () => void
    oncancel: () => void
    onenable: (site: Site, enabled: boolean) => void
    onremove: (site: Site) => void
    onopen: (url: string) => void
  }

  let { sites, settings, machine, editing, working = false, busy, onadd, onedit, ondraft, onsave, oncancel, onenable, onremove, onopen }: Props =
    $props()
</script>

<section class="sites" aria-label="Sites">
  {#if editing}
    <div class="scroll">
      <SiteForm draft={editing.draft} adding={editing.site === null} sharedPort={settings.port} busy={working} {ondraft} {onsave} {oncancel} />
    </div>
  {:else}
    <div class="bar">
      <p class="summary" data-testid="pages-sites-summary">{sitesSummary(sites.length)}</p>
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="tinted" hit disabled={working} onclick={onadd} data-testid="pages-add">
        <ui-icon slot="icon" name="plus"></ui-icon>
        Add Site
      </ui-button>
    </div>
    <div class="scroll">
      {#if sites.length > 0}
        <ul class="list" data-testid="pages-sites">
          {#each sites as site (site.id)}
            <SiteRow
              {site}
              addresses={addressesOf(site, settings, machine)}
              busy={busy(site)}
              disabled={working}
              onenable={(enabled) => onenable(site, enabled)}
              {onopen}
              onedit={() => onedit(site)}
              onremove={() => onremove(site)}
            />
          {/each}
        </ul>
      {:else}
        <ui-empty-state
          icon="globe"
          heading="No Sites"
          message="Add a folder of this machine as a site, and it is served on a port of its own, under a name, or both."
          data-testid="pages-empty"
        ></ui-empty-state>
      {/if}
    </div>
  {/if}
</section>

<style>
  .sites {
    height: 100%;
    display: flex;
    flex-direction: column;
    min-height: 0;
  }
  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 16px 4px;
    min-height: 44px;
  }
  .summary {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 4px 16px 16px;
  }
  .list {
    margin: 0;
    padding: 0;
    border-radius: var(--radius-lg);
    background: var(--bg-secondary);
  }
</style>
