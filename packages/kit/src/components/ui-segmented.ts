import { css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { repeat } from 'lit/directives/repeat.js'
import { styleMap } from 'lit/directives/style-map.js'
import { moveIndex, nextEnabled } from 'kit:keys'
import { define } from '../define'
import { FormControl, type ValidityResult } from '../form-control'
import { base } from '../styles'

/**
 * A segmented control (UISegmented Control): one choice from a few. A radio group: Tab reaches the selected segment,
 * the arrow keys move the selection, Home and End jump. A form control (see `FormControl`): the selected value is
 * submitted, and `required` keeps the form invalid until a segment is chosen.
 *
 * Each segment has the test id `<data-testid of the control>-<value>`. The segments are drawn 28 pt high; with `hit` they have a
 * 44 pt high hit area.
 *
 * @tag ui-segmented
 * @fires {CustomEvent<{ value: string }>} change - The person chose a segment. `value` is already updated.
 */
export class UiSegmented extends FormControl {
  static properties = {
    /** The segments: `[{ value, label, disabled? }]`. A JSON attribute works too. */
    options: { type: Array },
    /** The selected segment's value. */
    value: { type: String, reflect: true },
    /** Fill the width the parent gives it. */
    block: { type: Boolean, reflect: true },
    /** The form is invalid until a segment is chosen. */
    required: { type: Boolean, reflect: true },
    /** The test id; each segment's is this followed by `-<value>`. */
    testid: { type: String, attribute: 'data-testid' }
  }
  declare options: { value: string; label: string; disabled?: boolean }[]
  declare value: string
  declare block: boolean
  declare required: boolean
  declare testid: string | null
  private initial = ''

  static styles = [
    base,
    css`
      /* with hit, 44 pt high: layout space the control owns. Segments already tile it horizontally. */
      :host([hit]) {
        --ui-hit: 44px;
      }
      :host {
        display: inline-flex;
        align-items: center;
        vertical-align: middle;
        width: fit-content;
        height: fit-content; /* fixed-size, as in the shell: a flex column or a grid cell does not stretch it */
        min-height: var(--ui-hit, 0px);
      }
      :host([block]) {
        display: flex;
        width: 100%;
      }
      .group {
        position: relative;
        display: inline-grid;
        grid-template-columns: repeat(var(--count), minmax(0, 1fr));
        padding: 2px;
        min-height: 32px;
        border-radius: 9px;
        background: var(--fill-tertiary);
        isolation: isolate;
      }
      :host([block]) .group {
        display: grid;
        width: 100%;
      }
      .thumb {
        position: absolute;
        z-index: 0;
        top: 2px;
        bottom: 2px;
        left: 2px;
        width: calc((100% - 4px) / var(--count));
        border-radius: 7px;
        background: var(--segment-thumb);
        box-shadow:
          0 3px 8px rgb(0 0 0 / 0.12),
          0 3px 1px rgb(0 0 0 / 0.04),
          0 0 0 0.5px rgb(0 0 0 / 0.04);
        transform: translateX(calc(100% * var(--index)));
        transition: transform var(--duration) var(--ease-ios);
      }
      .group.none .thumb {
        display: none;
      }
      .segment {
        appearance: none;
        border: 0;
        background: none;
        position: relative;
        z-index: 1;
        padding: 0 14px;
        font: var(--type-footnote);
        font-weight: 500;
        letter-spacing: var(--tracking-footnote);
        color: var(--label);
        cursor: pointer;
        white-space: nowrap;
        transition: opacity var(--duration-fast);
      }
      .segment.selected {
        font-weight: 600;
      }
      /* a hit area 44 pt high around the drawn segment: vertical only, segments sit side by side with no gap */
      .segment::after {
        content: '';
        position: absolute;
        left: 0;
        right: 0;
        top: 50%;
        height: max(100%, var(--ui-hit, 0px));
        transform: translateY(-50%);
      }
      .segment:disabled {
        opacity: 0.4;
        cursor: default;
      }
      .segment:focus-visible {
        outline-offset: -2px;
        border-radius: 7px;
      }
      .segment:active:not(.selected):not(:disabled) {
        opacity: 0.5;
      }
      /* Hairline dividers between unselected segments. */
      .segment::before {
        content: '';
        position: absolute;
        left: 0;
        top: 25%;
        bottom: 25%;
        width: 1px;
        background: var(--separator);
        transition: opacity var(--duration-fast);
      }
      .segment.hide-divider::before {
        opacity: 0;
      }
    `
  ]

  constructor() {
    super()
    this.options = []
    this.value = ''
    this.block = false
    this.required = false
    this.testid = null
  }

  protected get formValue(): string | null {
    return this.selectedIndex >= 0 ? this.value : null
  }

  protected captureInitial(): void {
    this.initial = this.value
  }

  protected resetValue(): void {
    this.value = this.initial
  }

  protected restoreValue(state: string | File | FormData | null): void {
    if (typeof state === 'string') this.value = state
  }

  protected computeValidity(): ValidityResult {
    return this.required && this.selectedIndex < 0
      ? { flags: { valueMissing: true }, message: 'Choose one of the options.' }
      : { flags: {}, message: '' }
  }

  protected get validationAnchor(): HTMLElement | undefined {
    return this.renderRoot.querySelector<HTMLElement>('[role="radio"][tabindex="0"]') ?? undefined
  }

  private get off(): boolean {
    return this.isDisabled
  }

  private get selectedIndex(): number {
    return this.options.findIndex((s) => s.value === this.value)
  }

  private select(value: string): void {
    if (this.off || value === this.value) return
    this.value = value
    this.dispatchEvent(
      new CustomEvent('change', { detail: { value }, bubbles: true, composed: true })
    )
  }

  private onKeydown(e: KeyboardEvent): void {
    if (this.off || this.options.length === 0) return
    const from = (e.target as HTMLElement | null)?.closest<HTMLElement>('[role="radio"]')
    const current = from ? Number(from.dataset.index) : Math.max(0, this.selectedIndex)
    const target = moveIndex(this.options.length, current, e.key, 'both')
    if (target === null) return
    e.preventDefault()
    const dir = e.key === 'ArrowDown' || e.key === 'ArrowRight' ? 1 : -1
    const next =
      e.key === 'Home' || e.key === 'End'
        ? target
        : nextEnabled(
            this.options.map((s) => s.disabled === true),
            current,
            dir
          )
    const segment = this.options[next]
    if (!segment || segment.disabled) return
    this.select(segment.value)
    this.renderRoot.querySelector<HTMLElement>(`[data-index="${next}"]`)?.focus()
  }

  render(): TemplateResult {
    const selected = this.selectedIndex
    const index = Math.max(0, selected)
    const stop = selected >= 0 ? selected : 0
    const testid = this.testid
    return html`<div
      class="group ${selected < 0 ? 'none' : ''}"
      role="radiogroup"
      aria-label=${ifDefined(this.accessibleName)}
      aria-description=${ifDefined(this.description || undefined)}
      aria-disabled=${ifDefined(this.off ? 'true' : undefined)}
      style=${styleMap({ '--count': String(Math.max(1, this.options.length)), '--index': String(index) })}
      @keydown=${this.onKeydown}
    >
      <span class="thumb" aria-hidden="true"></span>
      ${repeat(
        this.options,
        (s) => s.value,
        (s, i) =>
          html`<button
            type="button"
            role="radio"
            class="segment ${s.value === this.value ? 'selected' : ''} ${i === index || i === index + 1 || i === 0 ? 'hide-divider' : ''}"
            data-testid=${ifDefined(testid ? `${testid}-${s.value}` : undefined)}
            data-index=${i}
            aria-checked=${s.value === this.value ? 'true' : 'false'}
            tabindex=${i === stop ? 0 : -1}
            ?disabled=${this.off || s.disabled}
            @click=${() => this.select(s.value)}
          >
            ${s.label}
          </button>`
      )}
    </div>`
  }
}

export type Segment = UiSegmented['options'][number]

define('ui-segmented', UiSegmented)
declare global {
  interface HTMLElementTagNameMap {
    'ui-segmented': UiSegmented
  }
}
