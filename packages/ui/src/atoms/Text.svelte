<!-- @atom Text — Dynamic Type text styles. -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  type TextStyle =
    | 'largeTitle'
    | 'title1'
    | 'title2'
    | 'title3'
    | 'headline'
    | 'body'
    | 'callout'
    | 'subheadline'
    | 'footnote'
    | 'caption1'
    | 'caption2'
    /** The instrument readout (Calculator class): large, light, tabular; `fit` shrinks it to the box. */
    | 'display'

  interface Props {
    style?: TextStyle
    color?: 'primary' | 'secondary' | 'tertiary' | 'tint' | 'destructive' | 'success' | 'warning' | 'inherit'
    weight?: 'regular' | 'medium' | 'semibold' | 'bold'
    as?: 'span' | 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'label' | 'div'
    mono?: boolean
    rounded?: boolean
    truncate?: boolean
    /** Maximum lines before an ellipsis (1 truncates; omit or 0 for unlimited). Overrides `truncate`. */
    lines?: number
    /** Let the person select and copy the text. */
    selectable?: boolean
    /** Equal-width digits, so a number that changes in place does not jitter. */
    tabular?: boolean
    /** Shrink the text to the width of its box (the `display` style). */
    fit?: boolean
    children: Snippet
  }

  let {
    style = 'body',
    color = 'primary',
    weight,
    as = 'span',
    mono = false,
    rounded = false,
    truncate = false,
    lines,
    selectable = false,
    tabular = false,
    fit = false,
    children
  }: Props = $props()

  const clamp = $derived(lines !== undefined && lines > 1 ? Math.min(lines, 20) : 0)
</script>

{#if fit}
  <span class="fit-box"><span class="ios-text {style} c-{color} fit-text" class:tabular>{@render children()}</span></span>
{:else}
<svelte:element
  this={as}
  class="ios-text {style} c-{color}"
  class:tabular
  class:selectable
  class:clamp={clamp > 0}
  style:--lines={clamp > 0 ? clamp : undefined}
  class:w-regular={weight === 'regular'}
  class:w-medium={weight === 'medium'}
  class:w-semibold={weight === 'semibold'}
  class:w-bold={weight === 'bold'}
  class:mono
  class:rounded
  class:truncate={lines === 1 || (truncate && lines === undefined)}
>
  {@render children()}
</svelte:element>
{/if}

<style>
  .ios-text {
    margin: 0;
  }
  .largeTitle { font: var(--type-large-title); font-weight: 700; letter-spacing: var(--tracking-large-title); }
  .title1 { font: var(--type-title1); letter-spacing: var(--tracking-title1); }
  .title2 { font: var(--type-title2); letter-spacing: var(--tracking-title2); }
  .title3 { font: var(--type-title3); letter-spacing: var(--tracking-title3); }
  .headline { font: var(--type-headline); letter-spacing: var(--tracking-body); }
  .body { font: var(--type-body); letter-spacing: var(--tracking-body); }
  .callout { font: var(--type-callout); letter-spacing: var(--tracking-callout); }
  .subheadline { font: var(--type-subheadline); letter-spacing: var(--tracking-subheadline); }
  .footnote { font: var(--type-footnote); letter-spacing: var(--tracking-footnote); }
  .caption1 { font: var(--type-caption1); }
  .caption2 { font: var(--type-caption2); letter-spacing: 0.07px; }

  .c-primary { color: var(--label); }
  .c-secondary { color: var(--label-secondary); }
  .c-tertiary { color: var(--label-tertiary); }
  .c-tint { color: var(--tint); }
  .c-destructive { color: var(--ios-red); }
  /* Green and orange are too light to read as text on a white card: darken toward the label colour. */
  .c-success { color: color-mix(in oklab, var(--ios-green) 55%, var(--label)); }
  .c-warning { color: color-mix(in oklab, var(--ios-orange) 52%, var(--label)); }
  .c-inherit { color: inherit; }

  .w-regular { font-weight: 400; }
  .w-medium { font-weight: 500; }
  .w-semibold { font-weight: 600; }
  .w-bold { font-weight: 700; }

  .mono { font-family: var(--font-mono); letter-spacing: 0; }
  .rounded { font-family: var(--font-rounded); }
  .display {
    font-family: var(--font-display);
    font-weight: 300;
    font-size: 56px;
    line-height: 1.1;
    letter-spacing: 0;
    font-variant-numeric: tabular-nums;
  }
  .tabular {
    font-variant-numeric: tabular-nums;
  }
  .selectable {
    -webkit-user-select: text;
    user-select: text;
  }
  .clamp {
    display: -webkit-box;
    -webkit-box-orient: vertical;
    -webkit-line-clamp: var(--lines);
    line-clamp: var(--lines);
    overflow: hidden;
    overflow-wrap: anywhere;
  }
  /* fit: the box is a size container and the readout is sized in its inline units, capped at the style's size. */
  .fit-box {
    display: block;
    container-type: inline-size;
    min-width: 0;
    overflow: hidden;
  }
  .fit-text {
    display: block;
    white-space: nowrap;
    font-size: min(56px, 16cqw);
  }
  .truncate {
    display: block;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
