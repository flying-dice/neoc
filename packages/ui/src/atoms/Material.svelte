<!-- @atom Material — blurred, translucent surface (UIBlurEffect). -->
<script lang="ts">
  import type { Snippet } from 'svelte'

  interface Props {
    thickness?: 'ultrathin' | 'thin' | 'regular' | 'thick' | 'chrome'
    radius?: string
    class?: string
    children: Snippet
  }

  let { thickness = 'regular', radius, class: klass = '', children }: Props = $props()
</script>

<div class="ios-material {thickness} {klass}" style:border-radius={radius}>
  {@render children()}
</div>

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
