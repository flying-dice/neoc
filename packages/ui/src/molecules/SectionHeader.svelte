<!--
  @molecule SectionHeader — the header of a collapsible list section: a disclosure button with the title and a
  chevron (`aria-expanded`). Atoms: Icon. Test id: `testid` on the button.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'

  interface Props {
    testid: string
    title: string
    expanded: boolean
    ontoggle: () => void
  }

  let { testid, title, expanded, ontoggle }: Props = $props()
</script>

<button type="button" class="ios-section-header" data-testid={testid} aria-expanded={expanded} onclick={ontoggle}>
  <span>{title}</span>
  <span class="chevron" class:open={expanded} aria-hidden="true">
    <Icon name="chevron.right" size={13} weight={2.6} />
  </span>
</button>

<style>
  .ios-section-header {
    appearance: none;
    border: 0;
    background: transparent;
    width: 100%;
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 8px;
    padding: 0 var(--row-inset) 7px;
    font: inherit;
    letter-spacing: inherit;
    text-transform: inherit;
    color: inherit;
    text-align: left;
    cursor: pointer;
  }
  .ios-section-header:focus-visible {
    outline: 2px solid var(--tint);
    outline-offset: -2px;
  }
  .chevron {
    display: flex;
    transition: transform var(--duration-fast) var(--ease-ios);
  }
  .chevron.open {
    transform: rotate(90deg);
  }
  @media (prefers-reduced-motion: reduce) {
    .chevron {
      transition: none;
    }
  }
</style>
