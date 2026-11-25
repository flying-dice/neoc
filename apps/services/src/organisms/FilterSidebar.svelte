<!-- @organism FilterSidebar — which services to show: all of them, or those running, stopped, failed or starting by themselves, each with how many; and under them which service manager this is -->
<script lang="ts">
  import type { Manager } from 'app:rpc'
  import { FILTERS, type Filter } from '../lib/format'

  interface Props {
    manager: Manager
    /** How many services each filter shows. */
    counts: Readonly<Record<Filter, number>>
    /** What is shown; nothing is marked when left out (a narrow window showing only this). */
    selected?: Filter
    onselect: (filter: Filter) => void
  }

  let { manager, counts, selected, onselect }: Props = $props()

  const KINDS: Readonly<Record<string, string>> = { systemd: 'systemd', launchd: 'launchd', windows: 'Windows Service Control Manager', mock: 'Test manager' }
</script>

<nav class="sidebar" aria-label="Filters">
  <div class="lists">
    <!-- a row's own name leaves its value out: it is named with its count, as it reads -->
    <ui-list variant="sidebar" selection="single" label="Services" data-testid="services-filters">
      <ui-section>
        {#each FILTERS as filter (filter.id)}
          <ui-row
            heading={filter.label}
            value={String(counts[filter.id])}
            label="{filter.label}, {counts[filter.id]}"
            icon={filter.icon}
            icon-color={filter.id === 'failed' ? 'red' : filter.id === 'running' ? 'green' : 'gray'}
            selected={selected === filter.id}
            onpress={() => onselect(filter.id)}
            data-testid="services-filter-{filter.id}"
          ></ui-row>
        {/each}
      </ui-section>
    </ui-list>
  </div>
  <div class="manager">
    <p class="about" data-testid="services-manager">
      <span class="name">{manager.available ? (KINDS[manager.kind] ?? manager.kind) : 'No service manager'}</span>
      <span class="where">{manager.available ? (manager.level === 'user' ? 'This user’s services' : 'This machine’s services') : 'Not available'}</span>
    </p>
  </div>
</nav>

<style>
  .sidebar {
    box-sizing: border-box;
    min-width: 0;
    height: 100%;
    display: flex;
    flex-direction: column;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding-top: 28px;
    background: var(--bg-secondary);
  }
  .lists {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 8px;
  }
  .manager {
    padding: 8px 12px 12px;
    border-top: 1px solid var(--separator);
  }
  .about {
    margin: 0;
    display: flex;
    flex-direction: column;
  }
  .name {
    font: var(--type-subheadline);
    font-weight: 600;
  }
  .where {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
</style>
