<!--
  @atom Toggle — UISwitch (default), a checkbox, or a pressed-state button. Test id: `testid` on the control.
  `switch` has role switch, `checkbox` role checkbox, `button` is a button with aria-pressed and a visible `title`.
-->
<script lang="ts">
  import { describe } from '../aria'
  interface Props {
    testid: string
    checked?: boolean
    disabled?: boolean
    /** On-state colour; defaults to system green (checkbox and button: the tint). */
    tint?: string
    /** Accessible name. */
    label?: string
    /** Accessible description (read after the name). */
    'aria-description'?: string
    /** `switch` (default), `checkbox`, or `button` (shows `title`). */
    variant?: 'switch' | 'checkbox' | 'button'
    /** The visible text of the `button` variant. */
    title?: string
    onchange?: (checked: boolean) => void
  }

  let {
    testid,
    checked = $bindable(false),
    disabled = false,
    tint,
    label,
    'aria-description': description,
    variant = 'switch',
    title,
    onchange
  }: Props = $props()

  function toggle(): void {
    if (disabled) return
    checked = !checked
    onchange?.(checked)
  }
</script>

{#if variant === 'button'}
  <button
    type="button"
    data-testid={testid}
    class="ios-toggle-button"
    class:on={checked}
    aria-pressed={checked}
    aria-label={label}
    {...describe(description)}
    {disabled}
    style:--toggle-tint={tint}
    onclick={toggle}
  >
    {title ?? label}
  </button>
{:else if variant === 'checkbox'}
  <button
    type="button"
    role="checkbox"
    data-testid={testid}
    class="ios-toggle-checkbox"
    class:on={checked}
    aria-checked={checked}
    aria-label={label}
    {...describe(description)}
    {disabled}
    style:--toggle-tint={tint}
    onclick={toggle}
  >
    <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <path d="M5 12.5l4.5 4.5L19 7" />
    </svg>
  </button>
{:else}
  <button
    type="button"
    role="switch"
    data-testid={testid}
    class="ios-toggle"
    class:on={checked}
    aria-checked={checked}
    aria-label={label}
    {...describe(description)}
    {disabled}
    style:--toggle-tint={tint}
    onclick={toggle}
  >
    <span class="knob"></span>
  </button>
{/if}

<style>
  .ios-toggle {
    appearance: none;
    border: 0;
    padding: 0;
    position: relative;
    flex: none;
    width: 51px;
    height: 31px;
    border-radius: 999px;
    background: var(--switch-off);
    cursor: pointer;
    transition: background-color var(--duration) var(--ease-ios);
  }
  .ios-toggle.on {
    background: var(--toggle-tint, var(--ios-green));
  }
  .ios-toggle:disabled,
  .ios-toggle-checkbox:disabled,
  .ios-toggle-button:disabled {
    opacity: 0.4;
    cursor: default;
  }
  .knob {
    position: absolute;
    top: 2px;
    left: 2px;
    width: 27px;
    height: 27px;
    border-radius: 999px;
    background: var(--knob);
    box-shadow:
      0 3px 8px rgb(0 0 0 / 0.15),
      0 3px 1px rgb(0 0 0 / 0.06),
      0 0 0 0.5px rgb(0 0 0 / 0.04);
    transition:
      transform var(--duration) var(--ease-ios),
      width var(--duration-fast) var(--ease-ios);
  }
  .on .knob {
    transform: translateX(20px);
  }
  /* Press stretches the knob, like iOS. */
  .ios-toggle:active:not(:disabled) .knob {
    width: 33px;
  }
  .ios-toggle.on:active:not(:disabled) .knob {
    transform: translateX(14px);
  }

  .ios-toggle-checkbox {
    appearance: none;
    flex: none;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border-radius: 50%;
    border: 2px solid var(--label-secondary);
    background: transparent;
    color: transparent;
    cursor: pointer;
    transition:
      background-color var(--duration-fast) var(--ease-ios),
      border-color var(--duration-fast) var(--ease-ios);
  }
  .ios-toggle-checkbox.on {
    border-color: transparent;
    background: var(--toggle-tint, var(--tint-fill));
    color: #fff;
  }

  .ios-toggle-button {
    appearance: none;
    min-height: 34px;
    padding: 0 14px;
    border: 0;
    border-radius: 999px;
    background: var(--fill-tertiary);
    color: var(--label);
    font: var(--type-subheadline);
    font-weight: 600;
    cursor: pointer;
    transition: background-color var(--duration-fast) var(--ease-ios);
  }
  .ios-toggle-button.on {
    background: var(--toggle-tint, var(--tint-fill));
    color: #fff;
  }
</style>
