<!--
  @page CalculatorPage — the Calculator window. Immediate-execution keypad
  logic like iOS; "=" is evaluated by the app's server module (which also keeps
  history), and agents can push values onto the display via the
  `show_on_calculator` tool.
-->
<script lang="ts">
  import { useAppApi } from '@agentos/sdk/app/client'
  import CalcDisplay from '../atoms/CalcDisplay.svelte'
  import Keypad from '../organisms/Keypad.svelte'
  import { formatDisplay } from '../../shared/evaluate'
  import type { KeyAction, Operator } from '../../shared/keys'

  const api = useAppApi()

  const SYMBOL: Record<Operator, string> = { '+': '+', '-': '−', '*': '×', '/': '÷' }

  let entry = $state('0') // what's being typed
  let accumulator = $state<number | null>(null)
  let pending = $state<Operator | null>(null)
  let fresh = $state(true) // next digit starts a new entry
  let caption = $state('')
  let error = $state(false)

  const shown = $derived(error ? 'Error' : formatEntry(entry))

  function formatEntry(text: string): string {
    if (text.endsWith('.') || /\.\d*0$/.test(text)) {
      const [int, frac] = text.split('.')
      return `${Number(int).toLocaleString('en-US')}.${frac ?? ''}`
    }
    return formatDisplay(Number(text))
  }

  async function compute(a: number, op: Operator, b: number): Promise<number> {
    const expression = `${a} ${op} ${b < 0 ? `(${b})` : b}`
    caption = `${formatDisplay(a)} ${SYMBOL[op]} ${formatDisplay(b)}`
    return api.invoke<number>('evaluate', expression)
  }

  async function press(action: KeyAction): Promise<void> {
    if (error && action.type !== 'clear') return
    try {
      switch (action.type) {
        case 'digit': {
          if (fresh) {
            entry = action.value === '.' ? '0.' : action.value
            fresh = false
          } else if (action.value === '.' ? !entry.includes('.') : entry.replace(/[-.]/g, '').length < 9) {
            entry = entry === '0' && action.value !== '.' ? action.value : entry + action.value
          }
          break
        }
        case 'operator': {
          const value = Number(entry)
          if (accumulator !== null && pending && !fresh) {
            accumulator = await compute(accumulator, pending, value)
            entry = String(accumulator)
          } else {
            accumulator = value
          }
          pending = action.value
          fresh = true
          break
        }
        case 'equals': {
          if (accumulator === null || !pending) break
          const result = await compute(accumulator, pending, Number(entry))
          entry = String(result)
          accumulator = null
          pending = null
          fresh = true
          break
        }
        case 'clear':
          // "C" clears the number being typed; "AC" (nothing typed) clears everything,
          // including a result left on the display.
          if (entry !== '0' && !fresh) {
            entry = '0'
          } else {
            entry = '0'
            accumulator = null
            pending = null
            caption = ''
          }
          error = false
          fresh = true
          break
        case 'negate':
          entry = entry.startsWith('-') ? entry.slice(1) : entry === '0' ? '0' : `-${entry}`
          break
        case 'percent':
          entry = String(Number(entry) / 100)
          break
        case 'backspace':
          if (!fresh) entry = entry.length > 1 && entry !== '-0' ? entry.slice(0, -1) : '0'
          break
      }
    } catch {
      error = true
      accumulator = null
      pending = null
      fresh = true
    }
  }

  // Values pushed by agents through the `show_on_calculator` tool.
  $effect(() =>
    api.on('display', (payload) => {
      const { expression, result } = payload as { expression: string; result: number }
      entry = String(result)
      caption = expression
      accumulator = null
      pending = null
      fresh = true
      error = false
    })
  )

  const KEYS: Record<string, KeyAction> = {
    '+': { type: 'operator', value: '+' },
    '-': { type: 'operator', value: '-' },
    '*': { type: 'operator', value: '*' },
    x: { type: 'operator', value: '*' },
    '/': { type: 'operator', value: '/' },
    '=': { type: 'equals' },
    Enter: { type: 'equals' },
    Escape: { type: 'clear' },
    '%': { type: 'percent' },
    Backspace: { type: 'backspace' }
  }

  function onkeydown(e: KeyboardEvent): void {
    const action = /^[0-9.]$/.test(e.key) ? ({ type: 'digit', value: e.key } as const) : KEYS[e.key]
    if (!action) return
    e.preventDefault()
    void press(action)
  }
</script>

<!--
  A focusable keyboard widget (digits, operators, Enter, Escape, Backspace),
  announced as an application; Svelte's checker doesn't treat that role as
  interactive, so its tabindex/key-handler warnings are suppressed here only.
-->
<!-- svelte-ignore a11y_no_noninteractive_tabindex, a11y_no_noninteractive_element_interactions -->
<div class="calculator" role="application" tabindex="0" aria-label="Calculator" data-testid="calculator" {onkeydown}>
  <div class="body">
    <CalcDisplay value={shown} {caption} />
    <!-- An operator stays highlighted only until the next number is started, like iPadOS. -->
    <Keypad pending={fresh ? pending : null} clearLabel={entry !== '0' && !fresh ? 'C' : 'AC'} onkey={press} />
  </div>
</div>

<style>
  .calculator {
    height: 100%;
    container-type: size;
    background: #000;
    outline: none;
    color-scheme: dark;
  }
  /*
   * Keys are circles sized to fit the window: by width (4 keys, 3 gaps,
   * 16px margins) or by height (5 rows, 4 gaps, plus ~150px for the handle
   * clearance and the display), capped at 96px. The display lines up with
   * the keypad when the window is wider than the keys need.
   */
  .body {
    --key-gap: 12px;
    --key: min(calc((100cqw - 68px) / 4), calc((100cqh - 202px) / 5), 96px);
    width: calc(var(--key) * 4 + var(--key-gap) * 3 + 32px);
    height: 100%;
    margin: 0 auto;
    display: flex;
    flex-direction: column;
    justify-content: flex-end;
    gap: 8px;
    padding-top: 28px;
  }
</style>
