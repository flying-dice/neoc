<!-- @molecule IconSlider — slider flanked by min/max glyphs (volume, brightness). Atoms: Slider, Icon. Test id: `testid` on the slider. -->
<script lang="ts">
  import Slider from '../atoms/Slider.svelte'
  import Icon from '../atoms/Icon.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    value?: number
    min?: number
    max?: number
    step?: number
    minIcon: IconName
    maxIcon: IconName
    tint?: string
    label?: string
    disabled?: boolean
  }

  let {
    testid,
    value = $bindable(50),
    min = 0,
    max = 100,
    step = 1,
    minIcon,
    maxIcon,
    tint,
    label,
    disabled = false
  }: Props = $props()
</script>

<div class="ios-icon-slider">
  <Icon name={minIcon} size={16} />
  <Slider {testid} bind:value {min} {max} {step} {tint} {label} {disabled} />
  <Icon name={maxIcon} size={22} />
</div>

<style>
  .ios-icon-slider {
    display: flex;
    align-items: center;
    gap: 12px;
    width: 100%;
    color: var(--label-secondary);
  }
</style>
