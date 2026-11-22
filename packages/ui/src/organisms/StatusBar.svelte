<!--
  @organism StatusBar — iPadOS status bar (time + date leading, indicators trailing).
  Molecules: StatusIndicators. Atoms: Icon.
  With `onsearch` a magnifier button sits before the indicators: system search, reachable whatever covers the screen.
  Test id: `${testid}-search` on that button.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import StatusIndicators from '../molecules/StatusIndicators.svelte'
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    /** Names the bar's own controls (the search button). */
    testid: string
    /** Open system search: shows the magnifier button. */
    onsearch?: () => void
    /** The keyboard shortcut that does the same, for assistive technology (`aria-keyshortcuts`: "Meta+K"). */
    searchShortcut?: string
    battery?: number
    /** Center slot: the shell's connection state. */
    center?: Snippet
  }

  let { testid, onsearch, searchShortcut, battery = 1, center }: Props = $props()

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

<div class="ios-statusbar">
  <span class="leading">
    <span class="time">{time}</span>
    <span class="date">{date}</span>
  </span>
  <span class="center">{#if center}{@render center()}{/if}</span>
  <span class="trailing">
    {#if onsearch}
      <button
        type="button"
        class="search"
        aria-label="Search"
        aria-haspopup="dialog"
        aria-keyshortcuts={searchShortcut}
        data-testid="{testid}-search"
        onclick={onsearch}
      >
        <Icon name="magnifyingglass" size={16} weight={2.6} />
      </button>
    {/if}
    <StatusIndicators {battery} />
  </span>
</div>

<style>
  .ios-statusbar {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    height: 28px;
    padding: 0 20px;
    font: 600 15px/1 var(--font-text);
    letter-spacing: -0.2px;
    /* light text over the wallpaper */
    color: #fff;
    text-shadow: 0 0 8px rgb(0 0 0 / 0.15);
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
    display: inline-flex;
    align-items: center;
    gap: 10px;
  }
  .search {
    appearance: none;
    position: relative;
    display: grid;
    place-items: center;
    width: 24px;
    height: 24px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: none;
    color: inherit;
    cursor: pointer;
  }
  /* the glyph is small, as the bar is: the target reaches the 44 px the design language asks for, sideways and down */
  .search::before {
    content: '';
    position: absolute;
    inset: -6px -10px -14px;
  }
  .search:active {
    opacity: 0.6;
  }
</style>
