<!--
  @molecule SessionToggle — a quiet capsule for a boolean session setting.
  Atoms: Icon. Test id: `testid` on the button.
-->
<script lang="ts">
  import { Icon } from '@agentos/ui/atoms'

  interface Props {
    testid: string
    label: string
    checked: boolean
    disabled?: boolean
    onchange: (checked: boolean) => void
  }

  let { testid, label, checked, disabled = false, onchange }: Props = $props()
</script>

<button
  type="button"
  class="pill"
  class:on={checked}
  role="switch"
  aria-checked={checked}
  data-testid={testid}
  {disabled}
  onclick={() => onchange(!checked)}
>
  {#if checked}<Icon name="checkmark" size={11} weight={2.8} />{/if}
  {label}
</button>

<style>
  .pill {
    appearance: none;
    border: 0;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 24px;
    padding: 0 9px;
    border-radius: 999px;
    background: transparent;
    color: var(--label-secondary);
    font: var(--type-caption1);
    font-weight: 500;
    cursor: pointer;
  }
  .pill:hover {
    background: var(--fill-tertiary);
    color: var(--label);
  }
  .on {
    color: var(--tint);
  }
  .pill:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
