<!--
  @atom Picture — an image with a required accessible alternative: `alt` text, or `decorative` (hidden from
  assistive technology). Only https URLs and same-origin paths load; a missing, refused or failed source shows the
  placeholder (a neutral tile with a glyph and the `placeholder` text). `size` makes it a square.
-->
<script lang="ts">
  import { safeImageSrc } from '../avatar'

  type Props =
    | { src?: string; alt: string; decorative?: false; fit?: 'fit' | 'fill'; size?: number; corner?: string; placeholder?: string; testid?: string }
    | { src?: string; alt?: string; decorative: true; fit?: 'fit' | 'fill'; size?: number; corner?: string; placeholder?: string; testid?: string }

  let { src, alt = '', decorative = false, fit = 'fit', size, corner, placeholder, testid }: Props = $props()

  const url = $derived(safeImageSrc(src))
  let failed = $state(false)
  $effect(() => {
    void url
    failed = false
  })
  const shown = $derived(url !== null && !failed)
</script>

<span
  class="ios-picture"
  class:placeholder={!shown}
  data-testid={testid}
  style:width={size ? `${size}px` : undefined}
  style:height={size ? `${size}px` : undefined}
  style:border-radius={corner}
  role={shown ? undefined : 'img'}
  aria-label={shown || decorative ? undefined : alt}
  aria-hidden={!shown && decorative ? 'true' : undefined}
>
  {#if shown}
    <img
      src={url}
      alt={decorative ? '' : alt}
      loading="lazy"
      referrerpolicy="no-referrer"
      style:object-fit={fit === 'fill' ? 'cover' : 'contain'}
      onerror={() => (failed = true)}
    />
  {:else}
    <svg viewBox="0 0 24 24" width="28" height="28" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
      <rect x="3.5" y="5" width="17" height="14" rx="2.5" /><circle cx="9" cy="10" r="1.6" /><path d="M4 17l5-4.5 4 3.5 3-2.5 4 3.5" />
    </svg>
    {#if placeholder}<span class="caption" aria-hidden="true">{placeholder}</span>{/if}
  {/if}
</span>

<style>
  .ios-picture {
    display: inline-flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 4px;
    max-width: 100%;
    overflow: hidden;
    vertical-align: middle;
  }
  .placeholder {
    min-width: 44px;
    min-height: 44px;
    background: var(--fill-tertiary);
    color: var(--label-secondary);
    border-radius: var(--radius-sm);
  }
  img {
    display: block;
    width: 100%;
    height: 100%;
    max-width: 100%;
  }
  .caption {
    font: var(--type-caption1);
    text-align: center;
    padding: 0 6px;
    color: var(--label);
  }
</style>
