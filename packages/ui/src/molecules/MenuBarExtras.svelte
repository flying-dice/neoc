<!--
  @molecule MenuBarExtras — trailing menu-bar cluster: battery, Wi-Fi, search,
  Control Centre and the clock. Atoms: Icon, BatteryIndicator.
  Test ids: `testid` on the cluster, `${testid}-search`, `${testid}-control-centre`.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import BatteryIndicator from '../atoms/BatteryIndicator.svelte'

  interface Props {
    testid: string
    battery?: number
    charging?: boolean
    wifi?: boolean
    onsearch?: () => void
    oncontrolcenter?: () => void
  }

  let { testid, battery = 1, charging = false, wifi = true, onsearch, oncontrolcenter }: Props = $props()

  let now = $state(new Date())
  $effect(() => {
    const t = setInterval(() => (now = new Date()), 1000 * 15)
    return () => clearInterval(t)
  })

  const clock = $derived(
    `${now.toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' })}  ${now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' })}`
  )
</script>

<div class="extras" data-testid={testid}>
  <span class="extra"><BatteryIndicator level={battery} {charging} /></span>
  {#if wifi}<span class="extra"><Icon name="wifi" size={15} weight={2.4} label="Wi-Fi connected" /></span>{/if}
  <button type="button" class="extra" aria-label="Search" data-testid="{testid}-search" onclick={onsearch}>
    <Icon name="magnifyingglass" size={14} weight={2.4} />
  </button>
  <button type="button" class="extra" aria-label="Control Centre" data-testid="{testid}-control-centre" onclick={oncontrolcenter}>
    <Icon name="switch.2" size={15} weight={2} />
  </button>
  <span class="clock">{clock}</span>
</div>

<style>
  .extras {
    display: flex;
    align-items: center;
    gap: 2px;
    font: 500 13px/1 var(--font-text);
    font-variant-numeric: tabular-nums;
  }
  .extra {
    appearance: none;
    border: 0;
    background: none;
    color: inherit;
    display: grid;
    place-items: center;
    height: 22px;
    padding: 0 7px;
    border-radius: 4px;
  }
  button.extra:hover {
    background: rgb(255 255 255 / 0.2);
  }
  .clock {
    padding: 0 8px;
    white-space: nowrap;
  }
</style>
