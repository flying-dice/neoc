<!--
  @molecule SearchPill — the Home Screen's "Search" capsule above the Dock, as on iOS: a button that opens system
  search. White on a dark translucent capsule, so the words and the glyph hold 4.5:1 on any wallpaper in either theme;
  with Reduce Transparency the capsule is solid.
  Atoms: Icon. Test id: `testid`.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    testid: string
    label?: string
    /** The keyboard shortcut that does the same, for assistive technology (`aria-keyshortcuts`: "Meta+K"). */
    shortcut?: string
    onclick?: () => void
  }

  let { testid, label = 'Search', shortcut, onclick }: Props = $props()
</script>

<button
  type="button"
  class="ios-search-pill"
  aria-haspopup="dialog"
  aria-keyshortcuts={shortcut}
  data-testid={testid}
  {onclick}
>
  <Icon name="magnifyingglass" size={13} weight={2.6} />
  <span>{label}</span>
</button>

<style>
  .ios-search-pill {
    appearance: none;
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 14px;
    border: 0;
    border-radius: var(--radius-pill);
    background: rgb(0 0 0 / 0.42);
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
    color: #fff;
    font: var(--type-footnote);
    font-weight: 600;
    letter-spacing: var(--tracking-footnote);
    cursor: pointer;
    transition: transform var(--duration-fast) var(--ease-ios);
  }
  /* the capsule is 30 tall: the target is the 44 the design language asks for */
  .ios-search-pill::before {
    content: '';
    position: absolute;
    inset: -7px 0;
  }
  .ios-search-pill:active {
    transform: scale(0.95);
    filter: brightness(0.85);
  }
  :global(.reduce-transparency) .ios-search-pill {
    background: #2c2c2e;
  }
</style>
