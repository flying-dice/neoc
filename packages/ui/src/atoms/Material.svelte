<!-- @atom Material — blurred, translucent surface (UIBlurEffect). -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    thickness?: 'ultrathin' | 'thin' | 'regular' | 'thick' | 'chrome'
    radius?: string
    as?: 'div' | 'section' | 'aside' | 'nav' | 'header' | 'footer'
    class?: string
    children: Snippet
  }

  let { thickness = 'regular', radius, as = 'div', class: klass = '', children }: Props = $props()
</script>

<svelte:element this={as} class="ios-material {thickness} {klass}" style:border-radius={radius}>
  {@render children()}
</svelte:element>

<style>
  .ios-material {
    -webkit-backdrop-filter: var(--material-blur);
    backdrop-filter: var(--material-blur);
  }
  .ultrathin { background: var(--material-ultrathin); }
  .thin { background: var(--material-thin); }
  .regular { background: var(--material-regular); }
  .thick { background: var(--material-thick); }
  .chrome { background: var(--material-chrome); }
</style>
