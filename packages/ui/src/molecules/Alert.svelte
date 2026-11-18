<!--
  @molecule Alert — UIAlertController: the centred `alert`, or the bottom `actionSheet` (a confirmation dialog on
  a narrow window). Actions have a role: `default`, `cancel` (bold, runs on Escape) and `destructive` (red); the
  `preferred` action is bold, takes initial focus and runs on Enter from a field. Optional prompt `fields` (text or
  secure) collect values that are handed to the action that closes the alert. Optional `children` render under the
  message (a long URL, a name that must be read in full: the text wraps anywhere). Focus is trapped and restored.
  Atoms: Scrim, Material, TextInput.
  Test ids: `testid` on the dialog, `${testid}-scrim`, `${testid}-action-<key or label slug>` on each action and
  `${testid}-field-<key>` on each field.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import { motion } from '../motion'
  import { fade, scale } from 'svelte/transition'
  import { cubicOut } from 'svelte/easing'
  import Scrim from '../atoms/Scrim.svelte'
  import Material from '../atoms/Material.svelte'
  import TextInput from '../atoms/TextInput.svelte'
  import { trapFocus } from '../focus'
  import type { AlertAction, AlertField } from '../types'

  interface Props {
    testid: string
    open?: boolean
    title: string
    message?: string
    actions?: AlertAction[]
    /** Prompt fields (a text input each). Their values go to the chosen action's `onclick`. */
    fields?: AlertField[]
    variant?: 'alert' | 'actionSheet'
    /** The dialog role: `alertdialog` (default for an alert) or `dialog`. */
    role?: 'alertdialog' | 'dialog'
    onclose?: () => void
    /** Extra content under the message. */
    children?: Snippet
  }

  let {
    testid,
    open = $bindable(false),
    title,
    message,
    actions = [{ label: 'OK', style: 'cancel' }],
    fields = [],
    variant = 'alert',
    role,
    onclose,
    children
  }: Props = $props()

  const slug = (label: string): string =>
    label.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '')

  const sheet = $derived(variant === 'actionSheet')
  const stacked = $derived(sheet || actions.length > 2)
  const hasPreferred = $derived(actions.some((a) => a.preferred))
  const cancel = $derived(actions.find((a) => a.style === 'cancel'))
  const preferred = $derived(actions.find((a) => a.preferred) ?? actions.find((a) => a.style !== 'cancel' && a.style !== 'destructive'))
  const rest = $derived(sheet ? actions.filter((a) => a !== cancel) : actions)
  const values: Record<string, string> = $state({})

  function run(action: AlertAction): void {
    action.onclick?.({ ...values })
    for (const k of Object.keys(values)) delete values[k]
    open = false
    onclose?.()
  }

  const dismissEscape = (): void => {
    if (cancel) run(cancel)
    else if (sheet || actions.length === 0) {
      open = false
      onclose?.()
    }
  }

  const keyOf = (a: AlertAction): string => a.key ?? slug(a.label)
  const isInitial = (a: AlertAction): boolean => fields.length === 0 && (a === preferred || (!preferred && a === cancel))
</script>

{#snippet button(action: AlertAction)}
  <button
    type="button"
    data-testid="{testid}-action-{keyOf(action)}"
    data-autofocus={isInitial(action) ? '' : undefined}
    class={action.style ?? 'default'}
    class:bold={action.preferred || (action.style === 'cancel' && !hasPreferred)}
    disabled={action.disabled}
    onclick={() => run(action)}
  >
    {action.label}
  </button>
{/snippet}

{#if open}
  <div class="ios-alert-layer" class:sheet>
    <Scrim testid="{testid}-scrim" opacity={sheet ? 0.4 : 0.2} onclick={() => sheet && dismissEscape()} />
    <div
      class="alert"
      class:sheet
      role={role ?? (sheet ? 'dialog' : 'alertdialog')}
      data-testid={testid}
      aria-modal="true"
      aria-label={title}
      aria-describedby={message ? `${testid}-message` : undefined}
      {@attach trapFocus({ onescape: dismissEscape })}
      in:scale={{ start: 1.15, duration: sheet ? 0 : motion(250), easing: cubicOut }}
      out:fade={{ duration: motion(150) }}
    >
      {#if sheet}
        <div class="group">
          <div class="solid">
            <div class="body">
              <h2>{title}</h2>
              {#if message}<p id="{testid}-message">{message}</p>{/if}
              {#if children}<div class="extra">{@render children()}</div>{/if}
            </div>
            <div class="actions stacked">
              {#each rest as action (action.label)}{@render button(action)}{/each}
            </div>
          </div>
        </div>
        {#if cancel}
          <div class="group cancel-group">
            <div class="solid">
              <div class="actions stacked">{@render button(cancel)}</div>
            </div>
          </div>
        {/if}
      {:else}
        <Material thickness="thick" radius="var(--radius-lg)">
          <div class="body">
            <h2>{title}</h2>
            {#if message}<p id="{testid}-message">{message}</p>{/if}
            {#if children}<div class="extra">{@render children()}</div>{/if}
            {#if fields.length}
              <form class="fields" onsubmit={(e) => { e.preventDefault(); if (preferred) run(preferred) }}>
                {#each fields as field, i (field.key)}
                  <div class="field">
                    <TextInput
                      testid="{testid}-field-{field.key}"
                      value={values[field.key] ?? ''}
                      oninput={(e) => (values[field.key] = e.currentTarget.value)}
                      type={field.secure ? 'password' : 'text'}
                      placeholder={field.placeholder}
                      aria-label={field.placeholder ?? field.key}
                      data-autofocus={i === 0 ? '' : undefined}
                      autocomplete="off"
                    />
                  </div>
                {/each}
                <button type="submit" hidden tabindex="-1" aria-hidden="true" data-testid="{testid}-submit"></button>
              </form>
            {/if}
          </div>
          <div class="actions" class:stacked>
            {#each actions as action (action.label)}{@render button(action)}{/each}
          </div>
        </Material>
      {/if}
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
  .ios-alert-layer.sheet {
    place-items: end center;
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
  .alert.sheet {
    width: min(420px, calc(100% - 16px));
    max-width: none;
    margin-bottom: 8px;
    display: flex;
    flex-direction: column;
    gap: 8px;
    overflow: visible;
    border-radius: 0;
  }
  .group {
    border-radius: var(--radius-lg);
    overflow: hidden;
  }
  .solid {
    background: var(--bg-elevated);
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
  .extra {
    margin-top: 8px;
    font: var(--type-footnote);
    letter-spacing: var(--tracking-footnote);
    overflow-wrap: anywhere;
  }
  .fields {
    display: flex;
    flex-direction: column;
    margin-top: 12px;
    border-radius: var(--radius-xs);
    overflow: hidden;
    border: var(--hairline) solid var(--separator);
    text-align: start;
  }
  .field {
    padding: 7px 8px;
    background: var(--bg-elevated);
  }
  .field + .field {
    border-top: var(--hairline) solid var(--separator);
  }
  .field :global(.ios-text-input) {
    font: var(--type-subheadline);
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
  .cancel-group .actions {
    box-shadow: none;
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
  button:disabled {
    color: var(--label-tertiary);
    cursor: default;
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
