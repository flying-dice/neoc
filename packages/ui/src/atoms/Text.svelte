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

  interface Props {
    style?: TextStyle
    color?: 'primary' | 'secondary' | 'tertiary' | 'tint' | 'destructive' | 'inherit'
    weight?: 'regular' | 'medium' | 'semibold' | 'bold'
    as?: 'span' | 'p' | 'h1' | 'h2' | 'h3' | 'h4' | 'label' | 'div'
    mono?: boolean
    rounded?: boolean
    truncate?: boolean
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
    children
  }: Props = $props()
</script>

<svelte:element
  this={as}
  class="ios-text {style} c-{color}"
  class:w-regular={weight === 'regular'}
  class:w-medium={weight === 'medium'}
  class:w-semibold={weight === 'semibold'}
  class:w-bold={weight === 'bold'}
  class:mono
  class:rounded
  class:truncate
>
  {@render children()}
</svelte:element>

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
  .c-inherit { color: inherit; }

  .w-regular { font-weight: 400; }
  .w-medium { font-weight: 500; }
  .w-semibold { font-weight: 600; }
  .w-bold { font-weight: 700; }

  .mono { font-family: var(--font-mono); letter-spacing: 0; }
  .rounded { font-family: var(--font-rounded); }
  .truncate {
    display: block;
    min-width: 0;
    overflow: hidden;
    white-space: nowrap;
    text-overflow: ellipsis;
  }
</style>
