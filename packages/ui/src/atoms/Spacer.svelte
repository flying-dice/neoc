<!-- @atom Spacer — flexible empty space that pushes its siblings apart; `min` is its smallest size in px. -->
<script lang="ts">
  interface Props {
    testid?: string
    min?: number
    style?: string
  }

  let { testid, min = 0, style }: Props = $props()
</script>

<div class="ios-spacer" data-testid={testid} aria-hidden="true" style:min-width="{min}px" style:min-height="{min}px" {style}></div>

<style>
  .ios-spacer {
    flex: 1 1 0;
  }
</style>
