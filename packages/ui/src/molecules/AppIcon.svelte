<!-- @molecule AppIcon — home-screen / dock app icon. Atoms: IconTile, Icon, Badge. Test id: `testid`. -->
<script lang="ts">
  import IconTile from '../atoms/IconTile.svelte'
  import Icon from '../atoms/Icon.svelte'
  import Badge from '../atoms/Badge.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    name: string
    icon: IconName
    /** Any CSS background, typically a two-stop gradient. */
    background?: string
    size?: number
    showLabel?: boolean
    badge?: number
    /** Small dot beneath the icon marking a running app. */
    running?: boolean
    onclick?: () => void
  }

  let {
    testid,
    name,
    icon,
    background = 'linear-gradient(180deg, #5ac8fa, #007aff)',
    size = 60,
    showLabel = true,
    badge,
    running = false,
    onclick
  }: Props = $props()
</script>

<!-- With a visible label the name comes from the content (so it matches what people see); icon-only uses aria-label. -->
<button type="button" class="ios-app-icon" aria-label={showLabel ? undefined : badge ? `${name}, ${badge} new` : name} data-testid={testid} {onclick}>
  <span class="plate">
    <IconTile {size} {background} shadow>
      <Icon name={icon} size={Math.round(size * 0.52)} weight={1.9} color="#fff" />
    </IconTile>
    {#if badge}<span class="badge" aria-hidden="true"><Badge value={badge} /></span>{/if}
  </span>
  {#if showLabel}<span class="label">{name}</span>{#if badge}<span class="sr-only">, {badge} new</span>{/if}{/if}
  {#if running}<span class="running" aria-label="Running"></span>{/if}
</button>

<style>
  .ios-app-icon {
    appearance: none;
    border: 0;
    background: none;
    padding: 0;
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    cursor: pointer;
    position: relative;
    transition: transform var(--duration-fast) var(--ease-ios);
  }
  .ios-app-icon:active {
    transform: scale(0.9);
    filter: brightness(0.85);
  }
  .plate {
    position: relative;
    display: block;
  }
  .badge {
    position: absolute;
    top: -6px;
    right: -6px;
  }
  .label {
    max-width: 84px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font: 400 12px/14px var(--font-text);
    color: #fff;
    text-shadow: 0 1px 2px rgb(0 0 0 / 0.35);
  }
  .running {
    position: absolute;
    bottom: -8px;
    width: 4px;
    height: 4px;
    border-radius: 50%;
    background: var(--label-secondary);
  }
  .sr-only {
    position: absolute;
    width: 1px;
    height: 1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
