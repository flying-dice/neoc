<!--
  @molecule Alert — UIAlertController's centred alert. Actions have a role: `default`, `cancel` (bold, runs on
  Escape) and `destructive` (red); the `preferred` action is bold and takes initial focus (else the first that is
  neither cancel nor destructive, else Cancel). Focus is trapped and restored.
  Atoms: Scrim, Material.
  Test ids: `testid` on the dialog, `${testid}-scrim`, `${testid}-action-<label slug>` on each action.
-->
<script lang="ts">
  import { motion } from '../motion'
  import { fade, scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Scrim from '../atoms/Scrim.svelte'
  import Material from '../atoms/Material.svelte'
  import { trapFocus } from '../focus'
  import type { AlertAction } from '../types'

  interface Props {
    testid: string
    open?: boolean
    title: string
    message?: string
    actions?: AlertAction[]
  }

  let {
    testid,
    open = $bindable(false),
    title,
    message,
    actions = [{ label: 'OK', style: 'cancel' }]
  }: Props = $props()

  const slug = (label: string): string =>
    label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  const stacked = $derived(actions.length > 2)
  const hasPreferred = $derived(actions.some((a) => a.preferred))
  const cancel = $derived(actions.find((a) => a.style === 'cancel'))
  const preferred = $derived(actions.find((a) => a.preferred) ?? actions.find((a) => a.style !== 'cancel' && a.style !== 'destructive'))
  const initial = $derived(preferred ?? cancel)

  function run(action: AlertAction): void {
    action.onclick?.()
    open = false
  }

  const dismissEscape = (): void => {
    if (cancel) run(cancel)
    else if (actions.length === 0) open = false
  }
</script>

{#if open}
  <div class="ios-alert-layer">
    <Scrim testid="{testid}-scrim" />
    <div
      class="alert"
      role="alertdialog"
      data-testid={testid}
      aria-modal="true"
      aria-label={title}
      aria-describedby={message ? `${testid}-message` : undefined}
      {@attach trapFocus({ onescape: dismissEscape })}
      in:scale={{ start: 1.15, duration: motion(250), easing: cubicOut }}
      out:fade={{ duration: motion(150) }}
    >
      <Material thickness="thick" radius="var(--radius-lg)">
        <div class="body">
          <h2>{title}</h2>
          {#if message}<p id="{testid}-message">{message}</p>{/if}
        </div>
        <div class="actions" class:stacked>
          {#each actions as action (action.label)}
            <button
              type="button"
              data-testid="{testid}-action-{slug(action.label)}"
              data-autofocus={action === initial ? '' : undefined}
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
    max-width: calc(100% - 32px);
    max-height: calc(100% - 32px);
    border-radius: var(--radius-lg);
    overflow: auto;
    color: var(--label);
    text-align: center;
    outline: 0;
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
    overflow-wrap: anywhere;
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
  button:focus-visible {
    outline-offset: -3px;
  }
  .bold {
    font-weight: 600;
  }
  .destructive {
    color: var(--ios-red);
  }
</style>
