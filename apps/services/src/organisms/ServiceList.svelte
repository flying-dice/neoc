<!-- @organism ServiceList — the services that are shown, each with the button that starts or stops it; the search that narrows them and the button that opens the form to register a new one -->
<script lang="ts">
  import type { Service } from 'app:rpc'
  import ServiceRow from '../molecules/ServiceRow.svelte'
  import { summary } from '../lib/format'

  interface Props {
    /** The name of what is shown. */
    title: string
    services: readonly Service[]
    /** The label of the service that is looked at. */
    selected?: string
    /** The words typed to narrow the list. */
    query: string
    /** Nothing is shown for lack of a filter or a search that matches, and not for lack of services. */
    filtered?: boolean
    /** Whether a call is in flight for a service. */
    busy: (service: Service) => boolean
    /** Show the way back to the filters (a window too narrow for them). */
    back?: boolean
    /** The manager can register a service. */
    canRegister?: boolean
    onopen: (service: Service) => void
    ontoggle: (service: Service) => void
    onquery: (query: string) => void
    onregister: () => void
    onback?: () => void
  }

  let { title, services, selected, query, filtered = false, busy, back = false, canRegister = true, onopen, ontoggle, onquery, onregister, onback }: Props = $props()
</script>

<section class="list" aria-label={title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the filters" onclick={onback} data-testid="services-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Filters
      </ui-button>
    {/if}
    <div class="heading">
      <h1 data-testid="services-title">{title}</h1>
      <p class="summary" data-testid="services-summary">{summary(services)}</p>
    </div>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="gray" square shape="circle" hit label="Register a service" disabled={!canRegister} onclick={onregister} data-testid="services-register">
      <ui-icon slot="icon" name="plus"></ui-icon>
    </ui-button>
  </header>
  <div class="search">
    <ui-text-field
      type="search"
      label="Search services"
      placeholder="Search"
      autocomplete="off"
      autocapitalize="off"
      value={query}
      oninput={(e: Event) => onquery((e.currentTarget as unknown as { value: string }).value)}
      data-testid="services-search"
    ></ui-text-field>
  </div>
  <div class="scroll">
    {#if services.length > 0}
      <ul class="services" data-testid="services-items">
        {#each services as service (service.label)}
          <ServiceRow
            {service}
            selected={selected === service.label}
            busy={busy(service)}
            onopen={() => onopen(service)}
            ontoggle={() => ontoggle(service)}
          />
        {/each}
      </ul>
    {:else if filtered}
      <ui-empty-state icon="magnifyingglass" heading="No Matching Services" message="Nothing here matches. Change the filter or the search." data-testid="services-none"></ui-empty-state>
    {:else}
      <ui-empty-state
        icon="gearshape.2"
        heading="No Services"
        message="The service manager has none. Register one with the plus button."
        data-testid="services-empty"
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
  .bar {
    display: flex;
    align-items: center;
    gap: 8px;
    min-height: 44px;
    padding: 0 16px 8px;
  }
  .heading {
    flex: 1;
    min-width: 0;
  }
  h1 {
    margin: 0;
    font: var(--type-title2);
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .summary {
    margin: 0;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .search {
    padding: 0 16px 8px;
  }
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 12px;
  }
  .services {
    margin: 0;
    padding: 0;
  }
</style>
