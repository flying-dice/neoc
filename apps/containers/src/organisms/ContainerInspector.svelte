<!-- @organism ContainerInspector — one container looked at: how it is doing, the buttons that start, stop, restart and remove it, and its logs (kept scrolled to the newest) or what it was made with -->
<script lang="ts">
  type Container = VosApp.Containers.Container
  type Details = VosApp.Containers.Details
  type Logs = VosApp.Containers.Logs
  import { StateDot } from '@vos/docker'
  import type { Action, Tab } from '../lib/containers.svelte'
  import { imageLabel, isUp, stateLabel, toneOf } from '../lib/format'

  interface Props {
    container: Container
    /** A call is in flight for it. */
    busy?: boolean
    tab: Tab
    /** Its logs, once they are read. */
    logs: Logs | null
    /** What it was made with, once that is read. */
    details: Details | null
    onact: (action: Action) => void
    ontab: (tab: Tab) => void
    onback: () => void
  }

  let { container, busy = false, tab, logs, details, onact, ontab, onback }: Props = $props()

  const TABS = [
    { value: 'logs', label: 'Logs' },
    { value: 'details', label: 'Details' }
  ]
  const up = $derived(isUp(container))
  let confirming = $state(false)

  let output = $state<HTMLElement>()
  // New output moves the newest line: keep it in view, unless the person has scrolled up to read. Read before the
  // text changes, so that the answer is about what was on screen.
  let atEnd = true
  $effect.pre(() => {
    void logs
    if (output) atEnd = output.scrollHeight - output.scrollTop - output.clientHeight < 24
  })
  const toNewest = (): void => output?.scrollTo({ top: output.scrollHeight })
  $effect(() => {
    void logs
    if (atEnd) toNewest()
  })
  // the lines change height when the fonts arrive, after the first scroll
  $effect(() => {
    void document.fonts.ready.then(() => atEnd && toNewest())
  })

  /** What the details say, each under its name; a list of nothing says so. */
  const facts = $derived.by((): [name: string, values: string[]][] => {
    if (!details) return []
    const or = (values: string[], none: string): string[] => (values.length > 0 ? values : [none])
    return [
      ['Image', [imageLabel(container.image)]],
      ['Command', [details.command]],
      ['Ports', or(container.ports, 'None published')],
      ['Restart', [details.restart]],
      ...(details.health ? ([['Health', [details.health]]] as [string, string[]][]) : []),
      ...(container.stack ? ([['Stack', [`${container.stack} · ${container.service}`]]] as [string, string[]][]) : []),
      ['Mounts', or(details.mounts, 'None')],
      ['Networks', or(details.networks, 'None')],
      ['Environment', or(details.environment, 'None')]
    ]
  })
</script>

<section class="inspector" aria-label={container.name}>
  <header class="bar">
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="plain" size="small" hit label="Back to the containers" onclick={onback} data-testid="containers-inspector-back">
      <ui-icon slot="icon" name="chevron.left"></ui-icon>
      Containers
    </ui-button>
    <div class="heading">
      <h1 data-testid="containers-inspector-name">{container.name}</h1>
      <p class="status" data-testid="containers-inspector-status">
        <StateDot tone={toneOf(container.state)} label={stateLabel(container.state)} />
        {container.status || stateLabel(container.state)}
      </p>
    </div>
  </header>
  <div class="actions">
    {#if busy}
      <ui-spinner label="Working on {container.name}" data-testid="containers-inspector-busy"></ui-spinner>
    {/if}
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      variant="gray"
      size="small"
      hit
      disabled={busy}
      onclick={() => onact(up ? 'stop' : 'start')}
      data-testid="containers-inspector-{up ? 'stop' : 'start'}"
    >
      <ui-icon slot="icon" name={up ? 'stop' : 'play'}></ui-icon>
      {up ? 'Stop' : 'Start'}
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button variant="gray" size="small" hit disabled={busy} onclick={() => onact('restart')} data-testid="containers-inspector-restart">
      Restart
    </ui-button>
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      class="remove"
      variant="plain"
      size="small"
      destructive
      hit
      disabled={busy || confirming}
      onclick={() => (confirming = true)}
      data-testid="containers-inspector-remove"
    >
      <ui-icon slot="icon" name="trash"></ui-icon>
      Remove
    </ui-button>
  </div>
  {#if confirming}
    <ui-alert
      open
      destructive
      action-label="Remove"
      heading="Remove {container.name}?"
      message={up ? 'It is running: it is stopped and removed. Its named volumes stay.' : 'Its named volumes stay.'}
      data-testid="containers-inspector-confirm"
      oncancel={() => (confirming = false)}
      onconfirm={() => {
        confirming = false
        onact('remove')
      }}
    ></ui-alert>
  {/if}
  <div class="tabs">
    <ui-segmented
      label="What to show"
      options={TABS}
      value={tab}
      hit
      onchange={(e) => ontab(e.detail.value as Tab)}
      data-testid="containers-inspector-tab"
    ></ui-segmented>
  </div>
  {#if tab === 'logs'}
    {#if logs === null}
      <div class="waiting"><ui-spinner label="Reading the logs"></ui-spinner></div>
    {:else if logs.text === ''}
      <ui-empty-state icon="doc" heading="No Logs" message="It has written nothing yet." data-testid="containers-no-logs"></ui-empty-state>
    {:else}
      {#if logs.truncated}
        <p class="note" data-testid="containers-logs-truncated">Only the end of the logs is shown.</p>
      {/if}
      <!-- a keyboard reaches the log to scroll it -->
      <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
      <pre class="output" bind:this={output} role="log" tabindex="0" aria-label="Logs of {container.name}" data-testid="containers-logs">{logs.text}</pre>
    {/if}
  {:else if details === null}
    <div class="waiting"><ui-spinner label="Reading the details"></ui-spinner></div>
  {:else}
    <!-- a keyboard reaches the details to scroll them -->
    <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
    <dl class="facts" tabindex="0" aria-label="Details of {container.name}" data-testid="containers-details">
      {#each facts as [name, values] (name)}
        <div class="fact">
          <dt>{name}</dt>
          {#each values as value, i (i)}
            <dd>{value}</dd>
          {/each}
        </div>
      {/each}
    </dl>
  {/if}
</section>

<style>
  .inspector {
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
    font: var(--type-title3);
    font-weight: 700;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .status {
    margin: 0;
    display: flex;
    align-items: center;
    gap: 6px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .actions {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: 8px;
    padding: 0 16px 8px;
  }
  .actions :global(.remove) {
    margin-inline-start: auto;
  }
  .tabs {
    padding: 4px 16px 8px;
  }
  .waiting {
    flex: 1;
    display: grid;
    place-items: center;
  }
  .note {
    margin: 0;
    padding: 0 16px 6px;
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  .output {
    flex: 1;
    min-height: 0;
    margin: 0 12px 12px;
    padding: 10px 12px;
    overflow: auto;
    border-radius: var(--radius-md);
    background: var(--fill-quaternary);
    font: var(--type-footnote);
    font-family: var(--font-mono);
    white-space: pre-wrap;
    overflow-wrap: anywhere;
    user-select: text;
  }
  .output:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: 1px;
  }
  .facts:focus-visible {
    outline: 2px solid var(--focus-ring);
    outline-offset: -2px;
  }
  .facts {
    flex: 1;
    min-height: 0;
    margin: 0;
    padding: 0 16px 12px;
    overflow-y: auto;
  }
  .fact {
    padding: 8px 0;
    border-bottom: 1px solid var(--separator);
  }
  dt {
    font: var(--type-footnote);
    color: var(--label-secondary);
  }
  dd {
    margin: 2px 0 0;
    font: var(--type-subheadline);
    font-family: var(--font-mono);
    overflow-wrap: anywhere;
    user-select: text;
  }
</style>
