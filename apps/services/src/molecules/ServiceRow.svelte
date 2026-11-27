<!-- @molecule ServiceRow — one service in a list: how it is doing in words and a dot, its name and what it is for, and the button that starts or stops it; a tap on the rest looks at it -->
<script lang="ts">
  type Service = VosApp.Services.Service
  import StatusDot from '../atoms/StatusDot.svelte'
  import { isUp, stateLabel, toneOf } from '../lib/format'

  interface Props {
    service: Service
    /** It is the one looked at. */
    selected?: boolean
    /** A call is in flight for it: the button gives way to a spinner. */
    busy?: boolean
    /** Look at it. */
    onopen: () => void
    /** Stop it when it is going, start it when it is not. */
    ontoggle: () => void
  }

  let { service, selected = false, busy = false, onopen, ontoggle }: Props = $props()

  const id = $derived(`services-item-${service.label}`)
  const up = $derived(isUp(service))
</script>

<li class="row" class:selected data-testid={id}>
  <button class="open" type="button" aria-current={selected ? 'true' : undefined} onclick={onopen} data-testid="{id}-open">
    <StatusDot tone={toneOf(service.state)} label={stateLabel(service.state)} />
    <span class="text">
      <span class="name">{service.label}</span>
      <span class="about">{stateLabel(service.state)}{service.description ? ` · ${service.description}` : ''}</span>
    </span>
    <ui-icon name="chevron.right" size={14}></ui-icon>
  </button>
  <span class="act">
    {#if busy}
      <ui-spinner label="Working on {service.label}" data-testid="{id}-busy"></ui-spinner>
    {:else}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="gray"
        square
        shape="circle"
        hit
        label="{up ? 'Stop' : 'Start'} {service.label}"
        onclick={ontoggle}
        data-testid="{id}-{up ? 'stop' : 'start'}"
      >
        <ui-icon slot="icon" name={up ? 'stop' : 'play'}></ui-icon>
      </ui-button>
    {/if}
  </span>
</li>

<style>
  .row {
    list-style: none;
    display: flex;
    align-items: center;
    gap: 4px;
    border-bottom: 1px solid var(--separator);
  }
  .open {
    flex: 1;
    min-width: 0;
    min-height: 56px;
    display: flex;
    align-items: center;
    gap: 12px;
    padding: 6px 4px;
    border: 0;
    border-radius: var(--radius-md);
    background: none;
    color: inherit;
    font: inherit;
    text-align: start;
    cursor: pointer;
  }
  .selected .open {
    background: var(--fill-tertiary);
  }
  .open:hover {
    background: var(--fill-quaternary);
  }
  .open:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }
  .name,
  .about {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .name {
    font: var(--type-body);
    font-weight: 600;
  }
  .about {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  ui-icon {
    flex: none;
    color: var(--label-tertiary);
  }
  /* the button and the spinner that stands in for it take the same room */
  .act {
    flex: none;
    width: 44px;
    height: 44px;
    display: grid;
    place-items: center;
  }
</style>
