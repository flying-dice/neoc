<!-- @atom Divider — a hairline separator (the `--separator` token); vertical inside a horizontal stack. -->
<script lang="ts">
  interface Props {
    testid?: string
    orientation?: 'horizontal' | 'vertical'
    style?: string
  }

  let { testid, orientation = 'horizontal', style }: Props = $props()
</script>

<div
  class="ios-divider"
  class:vertical={orientation === 'vertical'}
  data-testid={testid}
  role="separator"
  aria-orientation={orientation}
  {style}
></div>

<style>
  .ios-divider {
    flex: none;
    align-self: stretch;
    height: var(--hairline);
    background: var(--separator);
  }
  .vertical {
    width: var(--hairline);
    height: auto;
  }
</style>
