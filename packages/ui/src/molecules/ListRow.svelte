<!--
  @molecule ListRow — UITableViewCell / list cell: a `<button>` when it has `onclick`, a `<div>` otherwise (a row with
  a control in `trailing` is not itself tappable). Atoms: IconTile, Icon. Test id: `testid` on the row.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'
  import Icon from '../atoms/Icon.svelte'
  import IconTile from '../atoms/IconTile.svelte'
  import type { IconName } from '../icons'

  interface Props {
    testid: string
    title: string
    subtitle?: string
    /** Right-aligned secondary value (e.g. "On", a network name). */
    value?: string
    /** Settings-style rounded glyph tile. */
    icon?: IconName
    iconColor?: string
    chevron?: boolean
    selected?: boolean
    onclick?: () => void
    leading?: Snippet
    trailing?: Snippet
  }

  let {
    testid,
    title,
    subtitle,
    value,
    icon,
    iconColor = 'var(--ios-gray)',
    chevron = false,
    selected = false,
    onclick,
    leading,
    trailing
  }: Props = $props()

  const interactive = $derived(!!onclick)
</script>

<!-- svelte-ignore a11y_no_static_element_interactions (a row with a click handler renders as a <button>) -->
<svelte:element
  this={interactive ? 'button' : 'div'}
  type={interactive ? 'button' : undefined}
  class="ios-row"
  data-testid={testid}
  class:interactive
  class:selected
  aria-current={selected ? 'true' : undefined}
  {onclick}
>
  {#if leading}
    <span class="leading">{@render leading()}</span>
  {:else if icon}
    <IconTile background={iconColor}>
      <Icon name={icon} size={18} weight={2.2} color="#fff" />
    </IconTile>
  {/if}

  <span class="content">
    <span class="text">
      <span class="title">{title}</span>
      {#if subtitle}<span class="subtitle">{subtitle}</span>{/if}
    </span>
    <span class="accessory">
      {#if value}<span class="value">{value}</span>{/if}
      {#if trailing}{@render trailing()}{/if}
      {#if chevron}
        <span class="chevron" aria-hidden="true"><Icon name="chevron.right" size={14} weight={2.6} /></span>
      {/if}
    </span>
  </span>
</svelte:element>

<style>
  .ios-row {
    appearance: none;
    border: 0;
    margin: 0;
    width: 100%;
    display: flex;
    align-items: center;
    gap: 12px;
    min-height: var(--row-height);
    padding: 0 0 0 var(--row-inset);
    background: transparent;
    border-radius: var(--list-row-radius, 0);
    text-align: left;
    color: var(--label);
    font: var(--type-body);
    letter-spacing: var(--tracking-body);
    position: relative;
  }
  .interactive {
    cursor: pointer;
  }
  .interactive:active {
    background: var(--fill-secondary);
  }
  .selected,
  .selected:active {
    background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
    color: #fff;
  }
  .leading {
    flex: none;
    display: flex;
    align-items: center;
  }
  .content {
    flex: 1;
    min-width: 0;
    align-self: stretch;
    display: flex;
    align-items: center;
    gap: 8px;
    padding: 11px var(--row-inset) 11px 0;
    position: relative;
  }
  /* Inset hairline separator — starts at the text, not the icon. */
  .content::after {
    content: '';
    position: absolute;
    left: 0;
    right: 0;
    bottom: 0;
    height: var(--hairline);
    background: var(--list-separator, var(--separator));
  }
  .ios-row:last-child .content::after,
  .selected .content::after {
    display: none;
  }
  .text {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    gap: 1px;
  }
  .title {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .subtitle {
    font: var(--type-subheadline);
    letter-spacing: var(--tracking-subheadline);
    color: var(--label-secondary);
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .selected .subtitle,
  .selected .value,
  .selected .chevron {
    color: rgb(255 255 255 / 0.92);
  }
  .accessory {
    flex: none;
    display: flex;
    align-items: center;
    gap: 8px;
    position: relative;
  }
  .value {
    color: var(--label-secondary);
  }
  .chevron {
    display: flex;
    color: var(--label-tertiary);
  }
</style>
