<!-- @atom ColorSwatch — round colour chip; selectable when given onclick. Test id: `testid`. -->
<script lang="ts">
  interface Props {
    testid: string
    color: string
    label: string
    size?: number
    selected?: boolean
    onclick?: () => void
  }

  let { testid, color, label, size = 28, selected = false, onclick }: Props = $props()
</script>

<button
  type="button"
  class="swatch"
  data-testid={testid}
  class:selected
  aria-label={label}
  aria-pressed={onclick ? selected : undefined}
  title={label}
  disabled={!onclick}
  style:--swatch={color}
  style:width="{size}px"
  style:height="{size}px"
  {onclick}
></button>

<style>
  .swatch {
    appearance: none;
    border: 0;
    padding: 0;
    border-radius: 50%;
    background: var(--swatch);
    box-shadow: inset 0 0 0 0.5px rgb(0 0 0 / 0.12);
    cursor: pointer;
  }
  .swatch:disabled {
    cursor: default;
  }
  .selected {
    box-shadow:
      0 0 0 2px var(--bg-grouped-elevated-secondary),
      0 0 0 4px var(--swatch);
  }
</style>
