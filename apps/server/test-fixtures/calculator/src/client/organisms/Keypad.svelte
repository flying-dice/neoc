<!--
  @organism Keypad — the 4×5 calculator key grid. Atoms: CalcKey.
  Test ids: `calculator-keypad`, keys `calculator-key-<0-9|decimal|add|subtract|
  multiply|divide|equals|clear|negate|percent>`.
-->
<script lang="ts">
  import CalcKey from '../atoms/CalcKey.svelte'
  import type { KeyAction, Operator } from '../../shared/keys'

  interface Props {
    /** Operator awaiting its second operand (highlighted). */
    pending?: string | null
    /** "AC" when nothing typed yet, otherwise "C". */
    clearLabel?: 'AC' | 'C'
    onkey: (action: KeyAction) => void
  }

  let { pending = null, clearLabel = 'AC', onkey }: Props = $props()

  const op = (value: Operator): KeyAction => ({ type: 'operator', value })
  const digit = (value: string): KeyAction => ({ type: 'digit', value })
</script>

<div class="keypad" data-testid="calculator-keypad">
  <CalcKey testid="calculator-key-clear" variant="function" label={clearLabel === 'AC' ? 'AC, all clear' : 'Clear'} onclick={() => onkey({ type: 'clear' })}>{clearLabel}</CalcKey>
  <CalcKey testid="calculator-key-negate" variant="function" label="Negate" onclick={() => onkey({ type: 'negate' })}>±</CalcKey>
  <CalcKey testid="calculator-key-percent" variant="function" label="Percent" onclick={() => onkey({ type: 'percent' })}>%</CalcKey>
  <CalcKey testid="calculator-key-divide" variant="operator" label="Divide" active={pending === '/'} onclick={() => onkey(op('/'))}>÷</CalcKey>

  {#each ['7', '8', '9'] as d (d)}<CalcKey testid="calculator-key-{d}" label={d} onclick={() => onkey(digit(d))}>{d}</CalcKey>{/each}
  <CalcKey testid="calculator-key-multiply" variant="operator" label="Multiply" active={pending === '*'} onclick={() => onkey(op('*'))}>×</CalcKey>

  {#each ['4', '5', '6'] as d (d)}<CalcKey testid="calculator-key-{d}" label={d} onclick={() => onkey(digit(d))}>{d}</CalcKey>{/each}
  <CalcKey testid="calculator-key-subtract" variant="operator" label="Subtract" active={pending === '-'} onclick={() => onkey(op('-'))}>−</CalcKey>

  {#each ['1', '2', '3'] as d (d)}<CalcKey testid="calculator-key-{d}" label={d} onclick={() => onkey(digit(d))}>{d}</CalcKey>{/each}
  <CalcKey testid="calculator-key-add" variant="operator" label="Add" active={pending === '+'} onclick={() => onkey(op('+'))}>+</CalcKey>

  <CalcKey testid="calculator-key-0" wide label="0" onclick={() => onkey(digit('0'))}>0</CalcKey>
  <CalcKey testid="calculator-key-decimal" label="Decimal point" onclick={() => onkey(digit('.'))}>.</CalcKey>
  <CalcKey testid="calculator-key-equals" variant="operator" label="Equals" onclick={() => onkey({ type: 'equals' })}>=</CalcKey>
</div>

<style>
  /* Key size (--key) comes from CalculatorPage, which fits it to the window. */
  .keypad {
    display: grid;
    grid-template-columns: repeat(4, var(--key));
    grid-auto-rows: var(--key);
    gap: var(--key-gap);
    justify-content: center;
    padding: 0 16px 16px;
    font-size: calc(var(--key) * 0.42);
  }
</style>
