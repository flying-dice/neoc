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

<ui-row tappable {selected} onpress={onopen} data-testid={id}>
  <span slot="leading"><StatusDot tone={toneOf(service.state)} label={stateLabel(service.state)} /></span>
  <span class="text" data-testid="{id}-open">
    <span class="name">{service.label}</span>
    <span class="about">{stateLabel(service.state)}{service.description ? ` · ${service.description}` : ''}</span>
  </span>
  <span class="act" slot="trailing">
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
        title="{up ? 'Stop' : 'Start'} {service.label}"
        onclick={ontoggle}
        data-testid="{id}-{up ? 'stop' : 'start'}"
      >
        <ui-icon slot="icon" name={up ? 'stop' : 'play'}></ui-icon>
      </ui-button>
    {/if}
  </span>
</ui-row>

<style>
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 8px 0;
  }
  .name,
  .about {
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  /* on the selection's fill the row sets these two (a row of the kit does the same) */
  .name {
    color: var(--ui-value-main, var(--label));
    font: var(--type-body);
    font-weight: 600;
  }
  .about {
    font: var(--type-footnote);
    color: var(--ui-value-secondary, var(--label-secondary));
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
