<!--
  @molecule Label — a title with a glyph. `titleOnly` drops the glyph, `iconOnly` drops the visible title and keeps
  it as the accessible name. Atoms: Icon, Text. Not interactive.
-->
<script lang="ts">
  import Icon from '../atoms/Icon.svelte'
  import Text from '../atoms/Text.svelte'
  import type { IconName } from '../icons'

  interface Props {
    title: string
    icon?: IconName
    display?: 'titleAndIcon' | 'titleOnly' | 'iconOnly'
    style?: 'body' | 'callout' | 'subheadline' | 'footnote' | 'headline'
    color?: 'primary' | 'secondary' | 'tertiary' | 'tint' | 'destructive' | 'success' | 'warning'
    testid?: string
  }

  let { title, icon, display = 'titleAndIcon', style = 'body', color = 'primary', testid }: Props = $props()

  const showIcon = $derived(!!icon && display !== 'titleOnly')
  const showTitle = $derived(display !== 'iconOnly' || !icon)
  const glyph = $derived(style === 'footnote' ? 14 : style === 'subheadline' ? 16 : 18)
</script>

<span
  class="ios-label {color}"
  data-testid={testid}
  role={showTitle ? undefined : 'img'}
  aria-label={showTitle ? undefined : title}
>
  {#if showIcon && icon}<span class="glyph"><Icon name={icon} size={glyph} weight={2} /></span>{/if}
  {#if showTitle}<Text {style} {color}>{title}</Text>{/if}
</span>

<style>
  .ios-label {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }
  .glyph {
    display: inline-grid;
    flex: none;
    color: var(--tint);
  }
  .secondary .glyph {
    color: var(--label-secondary);
  }
  .tertiary .glyph {
    color: var(--label-tertiary);
  }
  .destructive .glyph {
    color: var(--ios-red);
  }
</style>
