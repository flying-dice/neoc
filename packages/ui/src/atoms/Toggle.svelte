<!-- @atom Toggle — UISwitch (role switch). Test id: `testid` on the control. -->
<script lang="ts">
  interface Props {
    testid: string
    checked?: boolean
    disabled?: boolean
    /** Accessible name. */
    label?: string
  }

  let { testid, checked = $bindable(false), disabled = false, label }: Props = $props()
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
  onclick={() => (checked = !checked)}
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
    background: var(--ios-green);
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
