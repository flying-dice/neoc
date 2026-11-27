<!-- @molecule ContainerRow — one container in a list: how it is doing, its name, image and Docker's words for it, its published ports, and the button that starts or stops it; a tap on the rest looks at it -->
<script lang="ts">
  type Container = VosApp.Containers.Container
  import { StateDot } from '@vos/docker'
  import { imageLabel, isUp, stateLabel, toneOf } from '../lib/format'

  interface Props {
    container: Container
    /** A call is in flight for it: the button gives way to a spinner. */
    busy?: boolean
    /** Look at it. */
    onopen: () => void
    /** Stop it when it is going, start it when it is not. */
    ontoggle: () => void
  }

  let { container, busy = false, onopen, ontoggle }: Props = $props()

  const id = $derived(`containers-item-${container.name}`)
  const up = $derived(isUp(container))
</script>

<li class="row" data-testid={id}>
  <button class="open" type="button" onclick={onopen} data-testid="{id}-open">
    <StateDot tone={toneOf(container.state)} label={stateLabel(container.state)} />
    <span class="text">
      <span class="name">{container.name}</span>
      <span class="about">{imageLabel(container.image)} · {container.status || stateLabel(container.state)}</span>
    </span>
    {#if container.ports.length > 0}
      <span class="ports" data-testid="{id}-ports">{container.ports.join(', ')}</span>
    {/if}
    <ui-icon name="chevron.right" size={14}></ui-icon>
  </button>
  <span class="act">
    {#if busy}
      <ui-spinner label="Working on {container.name}" data-testid="{id}-busy"></ui-spinner>
    {:else}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="gray"
        square
        shape="circle"
        hit
        label="{up ? 'Stop' : 'Start'} {container.name}"
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
  .ports {
    flex: 0 1 auto;
    min-width: 0;
    font: var(--type-footnote);
    font-family: var(--font-mono);
    color: var(--label-secondary);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
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
