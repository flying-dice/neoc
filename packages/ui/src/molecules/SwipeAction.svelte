<!--
  @molecule SwipeAction — one action revealed by swiping a list row (Delete, Pin, Archive). Atoms: Icon.
  Test id: `testid`. `role` picks the fill: `destructive` red, `normal` the tint, `neutral` grey; every fill holds
  4.5:1 against its white label in light and dark.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    title: string
    icon?: IconName
    role?: 'destructive' | 'normal' | 'neutral'
    disabled?: boolean
    /** Reachable by Tab only while the row's actions are open. */
    tabbable?: boolean
    onclick?: () => void
  }

  let { testid, title, icon, role = 'normal', disabled = false, tabbable = false, onclick }: Props = $props()
</script>

<button
  type="button"
  class="ios-swipe-action {role}"
  data-testid={testid}
  data-swipe-action
  tabindex={tabbable ? 0 : -1}
  {disabled}
  {onclick}
>
  {#if icon}<Icon name={icon} size={20} weight={2.2} />{/if}
  <span>{title}</span>
</button>

<style>
  .ios-swipe-action {
    appearance: none;
    border: 0;
    margin: 0;
    min-width: 76px;
    padding: 0 16px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    color: #fff;
    font: var(--type-subheadline);
    font-weight: 600;
    cursor: pointer;
    white-space: nowrap;
  }
  .normal {
    background: var(--tint-fill);
  }
  .destructive {
    background: light-dark(var(--ios-red), color-mix(in oklab, var(--ios-red) 60%, black));
  }
  .neutral {
    background: color-mix(in oklab, var(--ios-gray) 70%, black);
  }
  .ios-swipe-action:focus-visible {
    outline: 2px solid #fff;
    outline-offset: -4px;
  }
  .ios-swipe-action:disabled {
    opacity: 0.4;
    cursor: default;
  }
</style>
