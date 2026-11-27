<!-- @page ServicesPage — the whole app: the filters, the services of the one that is shown and the one that is looked at, in three columns (one or two in a narrow window); the form that registers a new one, the question before one is removed, and why there is no service manager to show; driven by one window's Services state -->
<script lang="ts">
  import FilterSidebar from '../organisms/FilterSidebar.svelte'
  import RegisterForm from '../organisms/RegisterForm.svelte'
  import ServiceDetail from '../organisms/ServiceDetail.svelte'
  import ServiceList from '../organisms/ServiceList.svelte'
  import type { Services } from '../lib/services.svelte'
  import { FILTERS, counts, isUp } from '../lib/format'

  interface Props {
    /** This window's state; the composition root builds it with the real backend and host, a story with a fake. */
    services: Services
    /** How often the services are asked of the operating system again, in milliseconds: another program changes them. */
    every?: number
  }

  let { services, every = 5000 }: Props = $props()

  // in a window too narrow for every column, the split view shows one: the kit says when, the same for every app
  let narrow = $state(false)
  // the column in front while narrow, when no service or form is: the filters or the list
  let browsing = $state<'sidebar' | 'content'>('content')
  const selected = $derived(services.selected)
  const title = $derived(FILTERS.find((f) => f.id === services.filter)?.label ?? 'Services')
  const unavailable = $derived(services.loaded && !services.manager.available)
  const showing = $derived(services.registering || selected !== undefined)
  // a service looked at, or the form, is in front; leaving them brings the list back
  const column = $derived(showing ? 'detail' : browsing)

  $effect(() => {
    void services.load()
    return services.listen()
  })

  // the operating system does not tell the app when another program changes a service: it is asked again
  $effect(() => {
    const timer = setInterval(() => void services.load(), every)
    return () => clearInterval(timer)
  })
</script>

<main class="app" data-testid="services-app">
  {#if !services.loaded}
    <div class="waiting">
      {#if !services.failure}<ui-spinner size="large" label="Asking the service manager" data-testid="services-loading"></ui-spinner>{/if}
    </div>
  {:else if unavailable}
    <div class="waiting">
      <ui-empty-state icon="gearshape.2" heading="No Service Manager" message={services.manager.problem} data-testid="services-unavailable"></ui-empty-state>
    </div>
  {:else}
    <ui-split-view {column} oncollapsedchange={(e: CustomEvent<{ collapsed: boolean }>) => (narrow = e.detail.collapsed)}>
      <div slot="sidebar">
        <FilterSidebar
          manager={services.manager}
          counts={counts(services.all)}
          selected={narrow ? undefined : services.filter}
          onselect={(filter) => {
            services.setFilter(filter)
            browsing = 'content'
          }}
        />
      </div>
      <div slot="content">
        <ServiceList
          {title}
          services={services.services}
          selected={narrow ? undefined : selected?.label}
          query={services.query}
          filtered={services.all.length > 0}
          busy={(service) => services.busy(service)}
          back={narrow}
          canRegister={services.manager.installOptions.length > 0}
          onopen={(service) => services.select(service)}
          ontoggle={(service) => void services.control(service, isUp(service) ? 'stop' : 'start')}
          onquery={(query) => services.setQuery(query)}
          onregister={() => services.openForm()}
          onback={() => (browsing = 'sidebar')}
        />
      </div>
      <div>
        {#if services.registering}
          <RegisterForm
            draft={services.draft}
            manager={services.manager}
            ready={services.ready}
            busy={services.adding}
            notice={services.notice?.label === '' ? services.notice : null}
            back={narrow}
            ondraft={(change) => services.edit(change)}
            onregister={() => void services.register()}
            oncancel={() => services.closeForm()}
          />
        {:else if selected}
          <ServiceDetail
            service={selected}
            manager={services.manager}
            busy={services.busy(selected)}
            notice={services.notice?.label === selected.label ? services.notice : null}
            back={narrow}
            oncontrol={(action) => void services.control(selected, action)}
            onconfigure={(change) => void services.configure(selected, change)}
            onremove={() => services.ask(selected)}
            onback={() => services.select(null)}
          />
        {:else}
          <div class="waiting">
            <ui-empty-state icon="gearshape.2" heading="No Service Selected" message="Choose a service to see how it is doing and to change it." data-testid="services-nothing"></ui-empty-state>
          </div>
        {/if}
      </div>
    </ui-split-view>
  {/if}
  {#if services.asking}
    <ui-alert
      open
      destructive
      action-label="Unregister"
      heading="Unregister {services.asking.label}?"
      message="The service is removed from the service manager and stops starting. The program it ran is kept."
      data-testid="services-confirm"
      onconfirm={() => void services.unregister()}
      oncancel={() => services.keep()}
    ></ui-alert>
  {/if}
  <!-- why the last call failed to get through (the host's sentence), whatever is in front; what the operating system answered is said at the service or the form -->
  {#if services.failure}
    <ui-banner tone="error" data-testid="services-failure">{services.failure}</ui-banner>
  {/if}
</main>

<style>
  .app {
    display: flex;
    flex-direction: column;
    height: 100%;
    background: var(--bg);
    color: var(--label);
    font: var(--type-body);
  }
  ui-split-view {
    flex: 1;
  }
  .waiting {
    flex: 1;
    height: 100%;
    display: grid;
    place-items: center;
    padding: 16px;
  }
</style>
