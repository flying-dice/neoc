<!--
  @atom IconTile — the rounded "squircle" plate behind app icons and
  Settings row glyphs. Content (usually an <Icon>) is passed as a snippet.
-->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    size?: number
    /** Any CSS background — solid colour or gradient. */
    background?: string
    shadow?: boolean
    children?: Snippet
  }

  let {
    size = 29,
    background = 'var(--ios-gray)',
    shadow = false,
    children
  }: Props = $props()
</script>

<span
  class="ios-icon-tile"
  class:shadow
  style:width="{size}px"
  style:height="{size}px"
  style:background
>
  {#if children}{@render children()}{/if}
</span>

<style>
  .ios-icon-tile {
    position: relative;
    flex: none;
    display: grid;
    place-items: center;
    border-radius: var(--radius-icon);
    color: #fff;
    overflow: hidden;
  }
  .shadow {
    box-shadow: var(--shadow-card);
  }
</style>
