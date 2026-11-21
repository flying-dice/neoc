<!--
  @organism StatusBar — iPadOS status bar (time + date leading, indicators trailing).
  Molecules: StatusIndicators.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import StatusIndicators from '../molecules/StatusIndicators.svelte'

  interface Props {
    /** Light text over wallpaper, or dark text over light content. */
    appearance?: 'light' | 'dark' | 'auto'
    battery?: number
    charging?: boolean
    wifi?: boolean
    /** Extra leading inset, e.g. to clear macOS traffic lights. */
    insetLeading?: number
    /** Center slot — e.g. an active agent / Dynamic Island–style activity. */
    center?: Snippet
  }

  let {
    appearance = 'light',
    battery = 1,
    charging = false,
    wifi = true,
    insetLeading = 0,
    center
  }: Props = $props()

  let now = $state(new Date())

  $effect(() => {
    const tick = setInterval(() => (now = new Date()), 1000 * 15)
    return () => clearInterval(tick)
  })

  const time = $derived(now.toLocaleTimeString([], { hour: 'numeric', minute: '2-digit' }))
  const date = $derived(
    now.toLocaleDateString([], { weekday: 'short', day: 'numeric', month: 'short' })
  )
</script>

<div class="ios-statusbar {appearance}" style:padding-left="{20 + insetLeading}px">
  <span class="leading">
    <span class="time">{time}</span>
    <span class="date">{date}</span>
  </span>
  <span class="center">{#if center}{@render center()}{/if}</span>
  <span class="trailing"><StatusIndicators {battery} {charging} {wifi} /></span>
</div>

<style>
  .ios-statusbar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: 28px;
    padding-right: 20px;
    font: 600 15px/1 var(--font-text);
    letter-spacing: -0.2px;
  }
  .light {
    color: #fff;
    text-shadow: 0 0 8px rgb(0 0 0 / 0.15);
  }
  .dark {
    color: #000;
  }
  .auto {
    color: var(--label);
  }
  .leading {
    display: flex;
    gap: 8px;
    font-variant-numeric: tabular-nums;
  }
  .date {
    font-weight: 500;
  }
  .trailing {
    justify-self: end;
  }
</style>
