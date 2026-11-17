<!--
  @molecule Alert — UIAlertController (.alert). Atoms: Scrim, Material.
  Test ids: `testid` on the dialog, `${testid}-scrim`, and
  `${testid}-action-<label slug>` on each action ("Reset" → `-action-reset`).
-->
<script lang="ts">
  import { motion } from '../motion'
  import { fade, scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Scrim from '../atoms/Scrim.svelte'
  import Material from '../atoms/Material.svelte'
  import type { AlertAction } from '../types'

  interface Props {
    testid: string
    open?: boolean
    title: string
    message?: string
    actions?: AlertAction[]
    onclose?: () => void
  }

  let {
    testid,
    open = $bindable(false),
    title,
    message,
    actions = [{ label: 'OK', style: 'cancel' }],
    onclose
  }: Props = $props()

  const slug = (label: string): string =>
    label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  const stacked = $derived(actions.length > 2)
  const hasPreferred = $derived(actions.some((a) => a.preferred))

  function run(action: AlertAction): void {
    action.onclick?.()
    open = false
    onclose?.()
  }

  function onkeydown(e: KeyboardEvent): void {
    if (!open || e.key !== 'Escape') return
    const cancel = actions.find((a) => a.style === 'cancel')
    if (cancel) run(cancel)
  }
</script>

<svelte:window {onkeydown} />

{#if open}
  <div class="ios-alert-layer">
    <Scrim testid="{testid}-scrim" opacity={0.2} />
    <div
      class="alert"
      role="alertdialog"
      data-testid={testid}
      aria-modal="true"
      aria-label={title}
      in:scale={{ start: 1.15, duration: motion(250), easing: cubicOut }}
      out:fade={{ duration: motion(150) }}
    >
      <Material thickness="thick" radius="var(--radius-lg)">
        <div class="body">
          <h2>{title}</h2>
          {#if message}<p>{message}</p>{/if}
        </div>
        <div class="actions" class:stacked>
          {#each actions as action (action.label)}
            <button
              type="button"
              data-testid="{testid}-action-{slug(action.label)}"
              class={action.style ?? 'default'}
              class:bold={action.preferred || (action.style === 'cancel' && !hasPreferred)}
              onclick={() => run(action)}
            >
              {action.label}
            </button>
          {/each}
        </div>
      </Material>
    </div>
  </div>
{/if}

<style>
  .ios-alert-layer {
    position: absolute;
    inset: 0;
    z-index: 1100;
    display: grid;
    place-items: center;
  }
  .alert {
    position: relative;
    width: 270px;
    border-radius: var(--radius-lg);
    overflow: hidden;
    color: var(--label);
    text-align: center;
  }
  .body {
    padding: 19px 16px 18px;
  }
  h2 {
    margin: 0;
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
  }
  p {
    margin: 2px 0 0;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
  }
  .actions {
    display: grid;
    grid-auto-flow: column;
    grid-auto-columns: 1fr;
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  .actions.stacked {
    grid-auto-flow: row;
  }
  button {
    appearance: none;
    border: 0;
    background: transparent;
    min-height: 44px;
    padding: 0 8px;
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    color: var(--tint);
    cursor: pointer;
  }
  button + button {
    box-shadow: calc(var(--hairline) * -1) 0 0 var(--separator);
  }
  .stacked button + button {
    box-shadow: 0 calc(var(--hairline) * -1) 0 var(--separator);
  }
  button:active {
    background: var(--fill-secondary);
  }
  .bold {
    font-weight: 600;
  }
  .destructive {
    color: var(--ios-red);
  }
</style>
