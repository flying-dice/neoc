<!-- @atom Toggle — UISwitch. Test id: `testid` on the switch. -->
<script lang="ts">
  interface Props {
    testid: string
    checked?: boolean
    disabled?: boolean
    /** On-state colour; defaults to system green. */
    tint?: string
    label?: string
    onchange?: (checked: boolean) => void
  }

  let { testid, checked = $bindable(false), disabled = false, tint, label, onchange }: Props = $props()

  function toggle(): void {
    if (disabled) return
    checked = !checked
    onchange?.(checked)
  }
</script>

<button
  type="button"
  role="switch"
  data-testid={testid}
  class="ios-toggle"
  class:on={checked}
  aria-checked={checked}
  aria-label={label}
  {disabled}
  style:--toggle-tint={tint}
  onclick={toggle}
>
  <span class="knob"></span>
</button>

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
  .ios-toggle:disabled {
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
</style>
