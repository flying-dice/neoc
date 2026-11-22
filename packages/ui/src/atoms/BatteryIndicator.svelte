<!-- @atom BatteryIndicator — status-bar battery glyph. -->
<script lang="ts">
  interface Props {
    /** 0–1 */
    level?: number
  }

  let { level = 1 }: Props = $props()
  const pct = $derived(Math.min(1, Math.max(0, level)))
  const fill = $derived(pct <= 0.2 ? 'var(--ios-red)' : 'currentColor')
</script>

<svg
  class="ios-battery"
  width="27"
  height="13"
  viewBox="0 0 27 13"
  role="img"
  aria-label="Battery {Math.round(pct * 100)}%"
>
  <rect x="0.5" y="0.5" width="22" height="12" rx="3.8" fill="none" stroke="currentColor" opacity="0.4" />
  <rect x="2" y="2" width={Math.max(1.5, 19 * pct)} height="9" rx="2.3" fill={fill} />
  <path d="M24.5 4.5v4c.8-.3 1.5-1.1 1.5-2s-.7-1.7-1.5-2z" fill="currentColor" opacity="0.45" />
</svg>

<style>
  .ios-battery {
    display: block;
    flex: none;
  }
</style>
