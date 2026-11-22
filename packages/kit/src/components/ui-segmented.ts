import { LitElement, css, html } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { repeat } from 'lit/directives/repeat.js'
import { moveIndex, nextEnabled } from 'kit:keys'
import { base } from '../styles'

/**
 * A segmented control (UISegmentedControl): one choice from a few. A radio group: Tab reaches the selected segment,
 * the arrow keys move the selection, Home and End jump.
 *
 * Each segment has the test id `<data-testid of the control>-<value>`.
 *
 * @tag ui-segmented
 * @fires {CustomEvent<{ value: string }>} change - The person chose a segment. `value` is already updated.
 */
export class UiSegmented extends LitElement {
  static formAssociated = true
  static shadowRootOptions = { ...LitElement.shadowRootOptions, delegatesFocus: true }
  static properties = {
    /** The segments: `[{ value, label, disabled? }]`. A JSON attribute works too. */
    options: { type: Array },
    /** The selected segment's value. */
    value: { type: String, reflect: true },
    /** The accessible name of the group. */
    label: { type: String },
    /** Read after the name. */
    description: { type: String },
    /** Fill the width the parent gives it. */
    block: { type: Boolean, reflect: true },
    /** Disables every segment. */
        disabled: { type: Boolean, reflect: true },
    /** The form field name; the selected value is submitted. */
    name: { type: String },
    /** The test id; each segment's is this followed by `-<value>`. */
    testid: { type: String, attribute: 'data-testid' }
  }
  declare options: { value: string; label: string; disabled?: boolean }[]
  declare value: string
  declare label: string
  declare description: string
  declare block: boolean
  declare disabled: boolean
  declare name: string
  declare testid: string | null
  private internals = this.attachInternals()
  private initial = ''
  private formDisabled = false

  static styles = [
    base,
    css`
      :host {
        display: inline-block;
        vertical-align: middle;
      }
      :host([block]) {
        display: block;
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
    this.label = ''
    this.description = ''
    this.block = false
    this.disabled = false
    this.name = ''
    this.testid = null
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.initial = this.value
  }

  private get off(): boolean {
    return this.disabled || this.formDisabled
  }

  private get selectedIndex(): number {
    return this.options.findIndex((s) => s.value === this.value)
  }

  private select(value: string): void {
    if (this.off || value === this.value) return
    this.value = value
    this.dispatchEvent(new CustomEvent('change', { detail: { value }, bubbles: true, composed: true }))
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

  protected updated(): void {
    this.internals.setFormValue(this.name ? this.value : null)
  }

  formResetCallback(): void {
    this.value = this.initial
  }

  formDisabledCallback(disabled: boolean): void {
    this.formDisabled = disabled
    this.requestUpdate()
  }

  render() {
    const selected = this.selectedIndex
    const index = Math.max(0, selected)
    const stop = selected >= 0 ? selected : 0
    const testid = this.testid
    return html`<div
      class="group ${selected < 0 ? 'none' : ''}"
      role="radiogroup"
      aria-label=${ifDefined(this.label || undefined)}
      aria-description=${ifDefined(this.description || undefined)}
      aria-disabled=${ifDefined(this.off ? 'true' : undefined)}
      style="--count:${Math.max(1, this.options.length)};--index:${index}"
      @keydown=${this.onKeydown}
    >
      <span class="thumb" aria-hidden="true"></span>
      ${repeat(
        this.options,
        (s) => s.value,
        (s, i) => html`<button
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

customElements.define('ui-segmented', UiSegmented)
declare global {
  interface HTMLElementTagNameMap {
    'ui-segmented': UiSegmented
  }
}
