<!-- @atom StateDot — how something in Docker is doing, as a coloured dot that says it in words to a screen reader -->
<script lang="ts">
  import type { Tone } from '../types'

  interface Props {
    tone: Tone
    /** What the colour means, in words. */
    label: string
  }

  let { tone, label }: Props = $props()
</script>

<span class="dot {tone}" role="img" aria-label={label}></span>

<style>
  .dot {
    flex: none;
    display: inline-block;
    box-sizing: border-box;
    width: 10px;
    height: 10px;
    border-radius: 50%;
  }
  .on {
    background: var(--ios-green);
  }
  /* not going is an empty ring: told apart from the others without its colour */
  .off {
    border: 2px solid var(--label-tertiary);
  }
  .busy {
    background: var(--ios-orange);
  }
  .bad {
    background: var(--ios-red);
  }
</style>
