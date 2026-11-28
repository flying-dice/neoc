<!-- @page CalculatorPage — the whole calculator: the display over the keypad, driven by one window's Calculator state -->
<script lang="ts">
  import Display from '../molecules/Display.svelte'
  import Keypad from '../organisms/Keypad.svelte'
  import type { Calculator } from '../lib/calculator.svelte'

  interface Props {
    /** This window's calculator; the composition root builds it with the real `api`, a story with a fake. */
    calculator: Calculator
  }

  let { calculator }: Props = $props()

  function onkeydown(event: KeyboardEvent): void {
    if (calculator.keyDown(event)) event.preventDefault()
  }
</script>

<svelte:document {onkeydown} onpointerdown={() => calculator.pointerDown()} />

<main class="calculator" data-theme="dark" data-testid="calculator-app">
  <div class="column">
    <Display display={calculator.display} caption={calculator.caption} />
    <Keypad clearLabel={calculator.clearLabel} pending={calculator.pending} onpress={(key) => calculator.press(key)} />
  </div>
  <!-- always on the page, so a screen reader hears the reason when it appears (a live region added with its text is often missed) -->
  <p class="announcer" aria-live="assertive" data-testid="calculator-announcer">{calculator.announcement}</p>
</main>

<style>
  /* the instrument is dark whatever the appearance, as the iPadOS Calculator is: the kit's tokens follow this data-theme */
  .calculator {
    box-sizing: border-box;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    align-items: center;
    height: 100%;
    /* the top clears the window's grabber, which the shell draws over the frame */
    padding: 28px 16px 16px;
    background: var(--bg);
    color: var(--label);
    color-scheme: dark;
  }
  /* one column the width and height of the window: the display keeps its own height and the keypad takes the rest, so
     the keys grow and shrink with the window in both directions */
  .column {
    flex: 1 1 auto;
    min-height: 0;
    width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .announcer {
    position: absolute;
    width: 1px;
    height: 1px;
    margin: -1px;
    overflow: hidden;
    clip-path: inset(50%);
    white-space: nowrap;
  }
</style>
