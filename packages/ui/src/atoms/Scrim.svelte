<!-- @atom Scrim — dimming layer behind modal presentations. Test id: `testid`. -->
<script lang="ts">
  import { motion } from '../motion'
  import { fade } from 'svelte/transition'

  interface Props {
    testid: string
    opacity?: number
    onclick?: () => void
  }

  let { testid, opacity = 0.4, onclick }: Props = $props()
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div
  class="ios-scrim"
  data-testid={testid}
  style:background="rgb(0 0 0 / {opacity})"
  transition:fade={{ duration: motion(220) }}
  {onclick}
></div>

<style>
  .ios-scrim {
    position: absolute;
    inset: 0;
  }
</style>
