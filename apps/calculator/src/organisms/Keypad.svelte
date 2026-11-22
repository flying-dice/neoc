<!-- @organism Keypad — the calculator's nineteen keys, as rounded kit buttons in a four-column grid that fills the room it is given -->
<script lang="ts">
  import { FILL, KEYS, type ClearLabel, type Key, type KeyDef, type Operator } from '../lib/keys'

  interface Props {
    /** What the clear key says and does: all clear, or clear the entry. */
    clearLabel: ClearLabel
    /** The operator waiting for its second number: its key is drawn pressed. */
    pending?: Operator
    onpress: (key: Key) => void
  }

  let { clearLabel, pending, onpress }: Props = $props()

  const nameOf = (k: KeyDef): string | undefined =>
    k.key === 'clear' ? (clearLabel === 'C' ? 'C, clear entry' : 'AC, all clear') : k.name
</script>

<div class="keys">
  {#each KEYS as k (k.id)}
    <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it -->
    <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
    <ui-button
      class:zero={k.id === '0'}
      size="fill"
      fill={FILL[k.face]}
      toggle={k.face === 'operator' && k.key !== '='}
      pressed={pending === k.key}
      label={nameOf(k)}
      onclick={() => onpress(k.key)}
      data-testid="calculator-key-{k.id}"
    >
      {k.key === 'clear' ? clearLabel : k.label}
    </ui-button>
  {/each}
</div>

<style>
  /* five equal rows of four: the keys are rounded rectangles, so they take whatever width and height the window gives
     the grid (a round key has to stay square, which is what held the keypad to one size) */
  .keys {
    flex: 1 1 auto; /* in the page's column, the height the display leaves */
    min-height: 0;
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    grid-template-rows: repeat(5, minmax(44px, 1fr));
    gap: 8px;
  }
  .keys > :global(ui-button) {
    font: var(--type-title2);
  }
  .keys > .zero {
    grid-column: span 2;
  }
  /* the wide 0's label sits over its first cell, in line with the 1 above it, not in the middle of the two */
  .zero::part(button) {
    justify-content: flex-start;
    padding-inline-start: calc(25% - 2px - 0.3em); /* the centre of the first of its two cells, less half a glyph */
  }
</style>
