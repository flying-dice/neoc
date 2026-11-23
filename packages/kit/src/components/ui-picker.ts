import { css, html, svg, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { repeat } from 'lit/directives/repeat.js'
import { define } from '../define'
import { FormControl, type ValidityResult } from '../form-control'
import { base } from '../styles'

/**
 * A picker (SwiftUI's `Picker` in its menu style, a pop-up button): one choice from a list too long, or too changeable,
 * for a `ui-segmented`. It shows the chosen option and opens the system's own menu of them, so the keyboard, type-ahead
 * and a screen reader work as they do on every select. A form control (see `FormControl`): the chosen value is
 * submitted, and `required` keeps the form invalid until an option is chosen.
 *
 * With a `value` no option has, it shows nothing chosen. Drawn 36 pt high; with `hit` it has a 44 pt high hit area.
 *
 * @tag ui-picker
 * @fires {CustomEvent<{ value: string }>} change - The person chose an option. `value` is already updated.
 * @csspart select - The inner select.
 */
export class UiPicker extends FormControl {
  static properties = {
    /** The options: `[{ value, label, disabled? }]`. A JSON attribute works too. */
    options: { type: Array },
    /** The chosen option's value. */
    value: { type: String, reflect: true },
    /** The form is invalid until an option is chosen. */
    required: { type: Boolean, reflect: true }
  }
  declare options: { value: string; label: string; disabled?: boolean }[]
  declare value: string
  declare required: boolean
  private initial = ''

  static styles = [
    base,
    css`
      /* with hit, 44 pt high: the strip around the 36 pt picker is layout space it owns */
      :host([hit]) {
        --ui-hit: 44px;
      }
      :host {
        display: inline-block;
        vertical-align: middle;
        min-width: 0;
        max-width: 100%;
        height: fit-content;
      }
      .box {
        position: relative;
        display: flex;
        align-items: center;
        width: 100%;
        min-height: var(--ui-hit, 0px);
      }
      select {
        appearance: none;
        display: block;
        width: 100%;
        min-width: 0;
        margin: 0;
        border: 0;
        /* the end clears the chevrons */
        padding-block: 0;
        padding-inline: 12px 30px;
        min-height: 36px;
        border-radius: var(--radius-md);
        outline-offset: 2px;
        background: var(--fill-tertiary);
        font: var(--type-body);
        letter-spacing: var(--tracking-body);
        color: var(--label);
        text-overflow: ellipsis;
        cursor: pointer;
      }
      select:disabled {
        cursor: default;
        opacity: 0.4;
      }
      /* the system's menu is drawn by the browser: its rows take the window's own colours */
      option {
        background: var(--bg-elevated);
        color: var(--label);
      }
      svg {
        position: absolute;
        inset-inline-end: 10px;
        width: 12px;
        height: 16px;
        fill: none;
        stroke: var(--label-secondary);
        stroke-width: 2;
        stroke-linecap: round;
        stroke-linejoin: round;
        pointer-events: none;
      }
      :host([disabled]) svg {
        opacity: 0.4;
      }
    `
  ]

  constructor() {
    super()
    this.options = []
    this.value = ''
    this.required = false
  }

  private get select(): HTMLSelectElement | null {
    return this.renderRoot.querySelector('select')
  }

  /** The options as given, or none when the property is not a list (a string an app or an agent passed is data). */
  private get choices(): { value: string; label: string; disabled?: boolean }[] {
    return Array.isArray(this.options) ? this.options : []
  }

  protected get validationAnchor(): HTMLElement | undefined {
    return this.select ?? undefined
  }

  protected get formValue(): string | null {
    return this.choices.some((o) => String(o.value) === this.value) ? this.value : null
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

  /** The inner select does the checking (`required`); the picker reports what it finds. */
  protected computeValidity(): ValidityResult {
    const select = this.select
    if (!select || select.validity.valid) return { flags: {}, message: '' }
    return {
      flags: { valueMissing: select.validity.valueMissing },
      message: select.validationMessage
    }
  }

  /** The select shows the option `value` names, or none: set after the options are drawn, which is when it can. */
  protected updated(): void {
    const select = this.select
    if (select && select.value !== this.value) {
      select.value = this.value
      if (select.value !== this.value) select.selectedIndex = -1
    }
    super.updated()
  }

  private onChange = (e: Event): void => {
    this.value = (e.target as HTMLSelectElement).value
    this.dispatchEvent(
      new CustomEvent('change', { detail: { value: this.value }, bubbles: true, composed: true })
    )
  }

  render(): TemplateResult {
    return html`<div class="box">
      <select
        part="select"
        aria-label=${ifDefined(this.accessibleName)}
        aria-description=${ifDefined(this.description || undefined)}
        ?disabled=${this.isDisabled}
        ?required=${this.required}
        @change=${this.onChange}
      >
        ${repeat(
          this.choices,
          (o) => String(o.value),
          (o) =>
            // the label is set as text, so no space written around it here becomes part of the option's name
            html`<option
              value=${String(o.value)}
              ?disabled=${o.disabled === true}
              .textContent=${o.label}
            ></option>`
        )}
      </select>
      <svg viewBox="0 0 12 16" aria-hidden="true">
        ${svg`<path d="M2.5 6 6 2.5 9.5 6M2.5 10 6 13.5 9.5 10" />`}
      </svg>
    </div>`
  }
}

/** One option of a `ui-picker`. */
export type PickerOption = UiPicker['options'][number]

define('ui-picker', UiPicker)
declare global {
  interface HTMLElementTagNameMap {
    'ui-picker': UiPicker
  }
}
