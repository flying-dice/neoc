type Screen = VosApp.Calculator.Screen
import { keyTyped, type ClearLabel, type Key, type Operator } from './keys'

/**
 * What the keypad needs of the backend: `press`, which answers each key with the next screen (the app's private method
 * `_press` on the bus: `App.svelte` sends it).
 */
export interface CalculatorRpc {
  press(params: VosApp.Calculator.PressParams): Promise<Screen>
}

/** What the keypad reads of a key going down on the keyboard (a `KeyboardEvent` has it). */
export interface KeyStroke {
  readonly key: string
  readonly metaKey: boolean
  readonly ctrlKey: boolean
  readonly altKey: boolean
}

/**
 * One window's calculator. The keypad's state is this window's own: the backend keeps nothing, it answers each key with
 * the next screen, and the next key is sent with the state that answer carried.
 */
export class Calculator {
  // `raw`: the screen is replaced whole by each answer, never edited in place
  #screen = $state.raw<Screen | null>(null)
  // the host or the connection, not the sum (a sum's own failure is the screen's `error`)
  #failure = $state<string | null>(null)

  /** The number on the display. */
  readonly display: string = $derived(this.#screen?.display ?? '0')
  /** The line over the display: the sum so far, why the sum failed, or why the key did not reach the backend. */
  readonly caption: string = $derived(this.#failure ?? this.#screen?.caption ?? '')
  readonly clearLabel: ClearLabel = $derived(this.#screen?.clear ?? 'AC')
  /** The operator waiting for its second number. */
  readonly pending: Operator | undefined = $derived(this.#screen?.pending)
  /** Why the sum failed, for a screen reader; empty when it has not. */
  readonly announcement: string = $derived(this.#screen?.error ?? '')

  readonly #rpc: CalculatorRpc
  // Keys are sent one after another, each with the state the one before answered with, so fast typing keeps its order;
  // a result an agent put on the Calculator takes its turn in the same line.
  #queue: Promise<void> = Promise.resolve()
  // the person reached a key with Tab and has typed nothing since
  #tabbed = false

  constructor(rpc: CalculatorRpc) {
    this.#rpc = rpc
  }

  /** Press a key. Resolves once its answer (or its failure) is on the screen; it never rejects. */
  press(key: Key): Promise<void> {
    this.#queue = this.#queue.then(async () => {
      try {
        this.#screen = await this.#rpc.press({ key, state: this.#screen?.state })
        this.#failure = null
      } catch (e) {
        this.#failure = e instanceof Error ? e.message : String(e)
      }
    })
    return this.#queue
  }

  /**
   * A key went down on the keyboard. True when the keypad took it (the caller then cancels the key's own action). Enter
   * is `=`, unless the person reached a key with Tab and typed nothing since: then Enter presses that key, as it does on
   * any focused button.
   */
  keyDown(stroke: KeyStroke): boolean {
    if (stroke.metaKey || stroke.ctrlKey || stroke.altKey) return false
    if (stroke.key === 'Tab') {
      this.#tabbed = true
      return false
    }
    if (stroke.key === 'Enter' && this.#tabbed) return false // the focused key's own Enter
    const key = keyTyped(stroke.key)
    if (key === undefined) return false
    this.#tabbed = false
    void this.press(key)
    return true
  }

  /** The pointer went down: the keyboard is no longer on a key the person tabbed to. */
  pointerDown(): void {
    this.#tabbed = false
  }
}
