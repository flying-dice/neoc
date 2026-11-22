<script lang="ts">
  import { api, type PressParams, type Screen } from 'app:rpc'

  type Key = PressParams['key']
  type Face = 'digit' | 'function' | 'operator'
  interface KeyDef {
    key: Key
    id: string
    label: string
    /** the accessible name, when the label is a glyph */
    name?: string
    face: Face
  }

  const digit = (d: Key): KeyDef => ({ key: d, id: d, label: d, face: 'digit' })
  const op = (key: Key, id: string, label: string, name: string): KeyDef => ({ key, id, label, name, face: 'operator' })

  // the iPadOS keypad, row by row; the clear key's label changes (AC or C) and is drawn from the screen
  const KEYS: KeyDef[] = [
    { key: 'clear', id: 'clear', label: 'AC', face: 'function' },
    { key: 'negate', id: 'negate', label: '±', name: 'Change sign', face: 'function' },
    { key: 'percent', id: 'percent', label: '%', name: 'Percent', face: 'function' },
    op('/', 'div', '÷', 'Divided by'),
    digit('7'),
    digit('8'),
    digit('9'),
    op('*', 'mul', '×', 'Times'),
    digit('4'),
    digit('5'),
    digit('6'),
    op('-', 'sub', '−', 'Minus'),
    digit('1'),
    digit('2'),
    digit('3'),
    op('+', 'add', '+', 'Plus'),
    digit('0'),
    { key: '.', id: 'dot', label: '.', name: 'Decimal point', face: 'digit' },
    op('=', 'eq', '=', 'Equals')
  ]
  // a palette token per face: the kit picks a text colour with 4.5:1 on it
  const FILL: Record<Face, string> = { digit: 'gray4', function: 'gray2', operator: 'orange' }

  // The keypad's state is this window's own: the backend keeps nothing, it answers each key with the next screen.
  let screen = $state<Screen | null>(null)
  let failure = $state<string | null>(null)

  // Keys are sent one after another, each with the state the one before answered with, so fast typing keeps its order.
  let queue: Promise<void> = Promise.resolve()
  function press(key: Key): void {
    queue = queue.then(async () => {
      try {
        screen = await api.press({ key, state: screen?.state })
        failure = null
      } catch (e) {
        // the host or the connection, not the sum (a sum's own failure is the screen's `error`)
        failure = e instanceof Error ? e.message : String(e)
      }
    })
  }

  const display = $derived(screen?.display ?? '0')
  const caption = $derived(failure ?? screen?.caption ?? '')
  const clearLabel = $derived(screen?.clear ?? 'AC')
  // the display shrinks to fit a long number in the 296 pt column, as the iPadOS display does
  const displaySize = $derived(Math.min(64, Math.floor((296 * 1.75) / Math.max(display.length, 1))))

  const nameOf = (k: KeyDef): string | undefined =>
    k.key === 'clear' ? (clearLabel === 'C' ? 'C, clear entry' : 'AC, all clear') : k.name

  // The keyboard: digits, operators, = or Enter, Backspace, Escape, %. Enter is `=`, unless the person reached a key
  // with Tab and typed nothing since: then Enter presses that key, as it does on any focused button.
  const TYPED: Record<string, Key> = {
    '+': '+',
    '-': '-',
    '*': '*',
    x: '*',
    '/': '/',
    '=': '=',
    Enter: '=',
    '%': 'percent',
    Backspace: 'backspace',
    Escape: 'clear',
    '.': '.',
    ',': '.'
  }
  let tabbed = false
  function onkeydown(event: KeyboardEvent): void {
    if (event.metaKey || event.ctrlKey || event.altKey) return
    if (event.key === 'Tab') {
      tabbed = true
      return
    }
    if (event.key === 'Enter' && tabbed) return // the focused key's own Enter
    const key = /^[0-9]$/.test(event.key) ? (event.key as Key) : TYPED[event.key]
    if (key === undefined) return
    event.preventDefault()
    tabbed = false
    press(key)
  }
</script>

<svelte:document {onkeydown} onpointerdown={() => (tabbed = false)} />

<main class="calculator" data-theme="dark" data-testid="calculator-web-app">
  <div class="column">
    <p class="caption" data-testid="calculator-web-caption">{caption}</p>
    <p
      class="display"
      style:font-size="{displaySize}px"
      aria-label="Result {display}"
      aria-live="polite"
      data-testid="calculator-web-display"
    >
      {display}
    </p>
    <div class="keys">
      {#each KEYS as k (k.id)}
        <!-- ui-button is a button (its own role and keys); Svelte cannot see inside it (packages/kit/README.md) -->
        <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
        <ui-button
          class:zero={k.id === '0'}
          shape={k.id === '0' ? 'capsule' : 'circle'}
          size="fill"
          fill={FILL[k.face]}
          toggle={k.face === 'operator' && k.key !== '='}
          pressed={screen?.pending === k.key}
          label={nameOf(k)}
          onclick={() => press(k.key)}
          data-testid="calculator-web-key-{k.id}"
        >
          {k.key === 'clear' ? clearLabel : k.label}
        </ui-button>
      {/each}
    </div>
  </div>
  <!-- always on the page, so a screen reader hears the reason when it appears (a live region added with its text is often missed) -->
  <p class="announcer" aria-live="assertive" data-testid="calculator-web-announcer">{screen?.error ?? ''}</p>
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
    padding: 12px;
    background: var(--bg);
    color: var(--label);
    color-scheme: dark;
  }
  /* one column, 296 pt wide (the narrowest window's content): the display's right edge is the keypad's */
  .column {
    width: 296px;
    max-width: 100%;
    display: flex;
    flex-direction: column;
    gap: 8px;
  }
  .caption {
    margin: 0;
    min-height: 1.4em;
    font: var(--type-body);
    color: var(--label-secondary);
    text-align: end;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .display {
    margin: 0;
    font-family: var(--font-display);
    font-weight: 300;
    line-height: 1.1;
    text-align: end;
    white-space: nowrap;
    overflow: hidden;
    font-variant-numeric: tabular-nums;
  }
  .keys {
    display: grid;
    grid-template-columns: repeat(4, 1fr);
    gap: 8px;
  }
  .keys > :global(ui-button) {
    aspect-ratio: 1;
    font: var(--type-title2);
  }
  .keys > .zero {
    grid-column: span 2;
    aspect-ratio: auto;
  }
  /* iPadOS sets the wide 0's label over its first circle, not in the middle of the capsule */
  .zero::part(button) {
    justify-content: flex-start;
    padding-inline-start: calc((296px - 3 * 8px) / 8 - 0.3em);
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
