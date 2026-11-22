<!-- @atom ActivityIndicator — UIActivityIndicatorView (8-tick spinner). -->
<script lang="ts">
  interface Props {
    size?: 'medium' | 'large'
    label?: string
  }

  let { size = 'medium', label = 'Loading' }: Props = $props()
  const px = $derived(size === 'large' ? 37 : 20)
</script>

<span class="ios-spinner" role="status" aria-label={label} style:width="{px}px" style:height="{px}px">
  <svg viewBox="0 0 20 20" width={px} height={px} aria-hidden="true">
    {#each Array.from({ length: 8 }, (_, i) => i) as i (i)}
      <rect
        x="9"
        y="1.5"
        width="2"
        height="5"
        rx="1"
        fill="var(--label-secondary)"
        opacity={0.15 + (i / 8) * 0.85}
        transform="rotate({i * 45} 10 10)"
      />
    {/each}
  </svg>
</span>

<style>
  .ios-spinner {
    display: inline-block;
    flex: none;
  }
  svg {
    display: block;
    animation: spin 0.8s steps(8) infinite;
  }
  @keyframes spin {
    to {
      transform: rotate(360deg);
    }
  }
  /* It says the app is busy: it keeps turning under Reduce Motion, as iOS's does (base.css stops every other animation). */
  @media (prefers-reduced-motion: reduce) {
    svg {
      animation-duration: 0.8s !important;
    }
  }
</style>
