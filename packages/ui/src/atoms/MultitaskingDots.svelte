<!--
  @atom MultitaskingDots — the iPadOS "···" window handle (Stage Manager / Split View). Test id: `testid`.
  The dots take `--window-grip` (3:1 on a light, dark or black body, as the resize corner does); the button is a
  44 x 24 hit area around them (the dots themselves are 5 px each).
-->
<script lang="ts">
  interface Props {
    testid: string
    label?: string
    /** The menu this button opens is showing. */
    expanded?: boolean
    onclick?: (e: MouseEvent) => void
  }

  let { testid, label = 'Window options', expanded = false, onclick }: Props = $props()
</script>

<button
  type="button"
  class="ios-mt-dots"
  aria-label={label}
  aria-haspopup="menu"
  aria-expanded={expanded}
  data-testid={testid}
  {onclick}
>
  <span></span><span></span><span></span>
</button>

<style>
  .ios-mt-dots {
    appearance: none;
    border: 0;
    background: none;
    box-sizing: border-box;
    display: inline-flex;
    justify-content: center;
    align-items: flex-start;
    gap: 4px;
    width: 44px;
    height: 24px;
    padding: 6px 0 0; /* the dots sit 6 px from the top edge of the window */
    border-radius: var(--radius-pill);
    cursor: pointer;
  }
  .ios-mt-dots:hover {
    background: var(--fill-tertiary);
  }
  span {
    width: 5px;
    height: 5px;
    border-radius: 50%;
    background: var(--window-grip);
  }
</style>
