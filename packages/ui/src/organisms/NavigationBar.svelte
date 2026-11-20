<!--
  @organism NavigationBar — UINavigationBar (inline or large title). Molecules: BackButton.
  Test ids: `testid` on the header, `${testid}-back` on the back button.

  Behaves like iOS 15+: only the bar row is sticky. It is transparent at the
  scroll edge and gains the chrome material once content scrolls beneath it.
  In large mode the large title and accessory scroll away with the content,
  and the title collapses into the bar.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import BackButton from '../molecules/BackButton.svelte'

  interface Props {
    testid: string
    title: string
    large?: boolean
    /** Renders a "‹ backLabel" button when set. */
    backLabel?: string
    onback?: () => void
    /** Always show the chrome material, even at the scroll edge. */
    material?: boolean
    leading?: Snippet
    trailing?: Snippet
    /** Rendered under the title, e.g. a SearchField or SegmentedControl. */
    accessory?: Snippet
  }

  let {
    testid,
    title,
    large = false,
    backLabel,
    onback,
    material = false,
    leading,
    trailing,
    accessory
  }: Props = $props()

  let bar: HTMLElement | undefined = $state()
  let largeTitle: HTMLElement | undefined = $state()
  let scrollTop = $state(0)

  /** Nearest scrolling ancestor — the bar sticks within it. */
  function scrollParent(el: HTMLElement): HTMLElement | null {
    let node = el.parentElement
    while (node) {
      const { overflowY } = getComputedStyle(node)
      if (overflowY === 'auto' || overflowY === 'scroll') return node
      if (overflowY === 'hidden') return null // clipped container (sheet, preview): static bar
      node = node.parentElement
    }
    return null
  }

  $effect(() => {
    const scroller = bar ? scrollParent(bar) : null
    if (!scroller) return undefined
    const onscroll = (): void => {
      scrollTop = scroller.scrollTop
    }
    onscroll()
    scroller.addEventListener('scroll', onscroll, { passive: true })
    return () => scroller.removeEventListener('scroll', onscroll)
  })

  const scrolled = $derived(scrollTop > 0)
  /** Large title has scrolled up under the bar → show the inline title. */
  const collapsed = $derived(
    large && largeTitle ? scrollTop > largeTitle.offsetHeight * 0.6 : !large
  )
</script>

<header class="ios-navbar" class:large data-testid={testid}>
  <div class="bar" class:material={material || scrolled} bind:this={bar}>
    <div class="side leading">
      {#if backLabel}<BackButton testid="{testid}-back" label={backLabel} onclick={onback} />{/if}
      {#if leading}{@render leading()}{/if}
    </div>
    <!-- In large mode the large title is the heading; this is a visual echo. -->
    <svelte:element
      this={large ? 'span' : 'h1'}
      class="inline-title"
      class:hidden={!collapsed}
      aria-hidden={large ? 'true' : undefined}
    >
      {title}
    </svelte:element>
    <div class="side trailing">
      {#if trailing}{@render trailing()}{/if}
    </div>
  </div>
  {#if large}<h1 class="large-title" bind:this={largeTitle}>{title}</h1>{/if}
  {#if accessory}<div class="accessory">{@render accessory()}</div>{/if}
</header>

<style>
  /* `contents` lets the bar stick within the scroll container, not the header. */
  .ios-navbar {
    display: contents;
  }
  .bar {
    position: sticky;
    top: 0;
    z-index: 10;
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    min-height: var(--nav-bar-height);
    padding: var(--nav-top-inset, 0px) var(--nav-inset, 16px) 0;
    gap: 8px;
    transition:
      background-color var(--duration-fast) ease,
      box-shadow var(--duration-fast) ease;
  }
  .material {
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
    background: var(--material-chrome);
    box-shadow: 0 var(--hairline) 0 var(--separator);
  }
  .side {
    display: flex;
    align-items: center;
    gap: 12px;
    color: var(--tint);
    min-width: 0;
  }
  .trailing {
    justify-content: flex-end;
  }
  .inline-title {
    margin: 0;
    font: var(--type-headline);
    letter-spacing: var(--tracking-body);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    transition: opacity var(--duration-fast) ease;
  }
  .inline-title.hidden {
    opacity: 0;
  }
  .large-title {
    margin: 0;
    padding: 2px var(--nav-inset, 16px) 8px;
    font: var(--type-large-title);
    font-weight: 700;
    letter-spacing: var(--tracking-large-title);
  }
  .accessory {
    padding: 0 var(--nav-inset, 16px) 10px;
  }
</style>
