<!-- @atom Scrim — dimming layer behind an alert. Test id: `testid`. -->
<script lang="ts">
  import { motion } from '../motion'
  import { fade } from 'svelte/transition'

  interface Props {
    testid: string
  }

  let { testid }: Props = $props()
</script>

<div
  class="ios-scrim"
  data-testid={testid}
  transition:fade={{ duration: motion(220) }}
></div>

<style>
  .ios-scrim {
    position: absolute;
    inset: 0;
    background: var(--dimming);
  }
</style>
