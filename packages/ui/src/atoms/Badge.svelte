<!-- @atom Badge — red count / dot badge. -->
<script lang="ts">
  interface Props {
    /** Number or short text. Omit for a dot. */
    value?: number | string
    color?: string
    max?: number
  }

  let { value, color = 'var(--ios-red)', max = 99 }: Props = $props()
  const text = $derived(typeof value === 'number' && value > max ? `${max}+` : value)
</script>

<span class="ios-badge" class:dot={text === undefined} style:background={color}>{text ?? ''}</span>

<style>
  .ios-badge {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 20px;
    height: 20px;
    padding: 0 6px;
    border-radius: 999px;
    color: #fff;
    font: var(--type-footnote);
    font-weight: 500;
    font-variant-numeric: tabular-nums;
    line-height: 1;
  }
  .dot {
    min-width: 10px;
    width: 10px;
    height: 10px;
    padding: 0;
  }
</style>
