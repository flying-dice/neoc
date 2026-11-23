<!-- @organism ContainerList — the containers that are shown, each with the button that starts or stops it; for a Compose stack, also where it came from and the buttons that act on all of it -->
<script lang="ts">
  import type { Container, Stack } from 'app:rpc'
  import ContainerRow from '../molecules/ContainerRow.svelte'
  import type { Action } from '../lib/containers.svelte'
  import { runningSummary } from '../lib/format'

  interface Props {
    /** The name of what is shown. */
    title: string
    containers: readonly Container[]
    /** The stack that is shown, when one is: it can be acted on as a whole. */
    stack?: Stack
    /** A call is in flight for the stack. */
    stackBusy?: boolean
    /** Whether a call is in flight for a container. */
    busy: (container: Container) => boolean
    /** Show the way back to the stacks (a window too narrow for both). */
    back?: boolean
    onopen: (container: Container) => void
    /** Stop a container that is going, start one that is not. */
    ontoggle: (container: Container) => void
    onstack?: (stack: Stack, action: Action) => void
    onback?: () => void
  }

  let { title, containers, stack, stackBusy = false, busy, back = false, onopen, ontoggle, onstack, onback }: Props = $props()

  // taking a stack down removes its containers: the name of the stack the person has been asked about
  let asking = $state<string | null>(null)
  const confirming = $derived(stack !== undefined && asking === stack.name)
</script>

<section class="list" aria-label={title}>
  <header class="bar">
    {#if back}
      <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="plain" size="small" hit label="Back to the stacks" onclick={onback} data-testid="containers-back">
        <ui-icon slot="icon" name="chevron.left"></ui-icon>
        Stacks
      </ui-button>
    {/if}
    <div class="heading">
      <h1 data-testid="containers-title">{title}</h1>
      <p class="summary" data-testid="containers-summary">
        {runningSummary(containers)}{#if stack?.folder}{' · '}<span class="folder">{stack.folder}</span>{/if}
      </p>
    </div>
  </header>
  {#if stack}
    <div class="actions" data-testid="containers-stack-actions">
      {#if stackBusy}
        <ui-spinner label="Working on {stack.name}" data-testid="containers-stack-busy"></ui-spinner>
      {/if}
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="gray"
        size="small"
        hit
        disabled={stackBusy || stack.running === stack.total}
        onclick={() => onstack?.(stack, 'start')}
        data-testid="containers-stack-start"
      >
        <ui-icon slot="icon" name="play"></ui-icon>
        Start
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        variant="gray"
        size="small"
        hit
        disabled={stackBusy || stack.running === 0}
        onclick={() => onstack?.(stack, 'stop')}
        data-testid="containers-stack-stop"
      >
        <ui-icon slot="icon" name="stop"></ui-icon>
        Stop
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button variant="gray" size="small" hit disabled={stackBusy} onclick={() => onstack?.(stack, 'restart')} data-testid="containers-stack-restart">
        Restart
      </ui-button>
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
      <ui-button
        class="remove"
        variant="plain"
        size="small"
        destructive
        hit
        disabled={stackBusy || confirming}
        onclick={() => (asking = stack.name)}
        data-testid="containers-stack-remove"
      >
        Remove
      </ui-button>
    </div>
    {#if confirming}
      <ui-alert
        open
        destructive
        action-label="Remove"
        heading="Remove {stack.name}?"
        message="Its containers and networks are removed, running or not. Its volumes stay."
        data-testid="containers-stack-confirm"
        oncancel={() => (asking = null)}
        onconfirm={() => {
          asking = null
          onstack?.(stack, 'remove')
        }}
      ></ui-alert>
    {/if}
  {/if}
  <div class="scroll">
    {#if containers.length > 0}
      <ul class="containers" data-testid="containers-items">
        {#each containers as container (container.id)}
          <ContainerRow {container} busy={busy(container)} onopen={() => onopen(container)} ontoggle={() => ontoggle(container)} />
        {/each}
      </ul>
    {:else}
      <ui-empty-state
        icon="rectangle.stack"
        heading="No Containers"
        message="Docker has none yet. One appears here when it is made with docker run or docker compose up."
        data-testid="containers-empty"
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
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .folder {
    font-family: var(--font-mono);
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
  .scroll {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    padding: 0 12px;
  }
  .containers {
    margin: 0;
    padding: 0;
  }
</style>
