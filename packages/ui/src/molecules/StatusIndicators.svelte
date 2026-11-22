<!-- @molecule StatusIndicators — status-bar trailing cluster (Wi-Fi, battery). Atoms: Icon, BatteryIndicator. -->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import BatteryIndicator from '../atoms/BatteryIndicator.svelte'

  interface Props {
    battery?: number
  }

  let { battery = 1 }: Props = $props()
</script>

<span class="ios-status-indicators">
  <Icon name="wifi" size={17} weight={2.4} label="Wi-Fi connected" />
  <span class="pct">{Math.round(battery * 100)}%</span>
  <BatteryIndicator level={battery} />
</span>

<style>
  .ios-status-indicators {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    font: 600 15px/1 var(--font-text);
    letter-spacing: -0.2px;
    font-variant-numeric: tabular-nums;
  }
  .pct {
    font-weight: 500;
    font-size: 13px;
  }
</style>
