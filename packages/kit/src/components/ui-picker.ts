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
 * With a `value` no option has, it shows nothing chosen. Drawn 36 pt high inside a 44 pt high target, all
 * of which opens the menu.
 *
 * @tag ui-picker
 * @fires {Event} input - The person chose an option (the native event, passed through). `value` is already updated.
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
    required: { type: Boolean, reflect: true },
    /** `field` (default): a rounded filled field. `bare`: no box, for use inside a `ui-row`. */
    variant: { type: String, reflect: true },
    /** Shows the picker as invalid (`aria-invalid`): a red outline, or a red choice for a `bare` picker in a row. */
    invalid: { type: Boolean, reflect: true }
  }
  declare options: { value: string; label: string; disabled?: boolean }[]
  declare value: string
  declare required: boolean
  declare variant: 'field' | 'bare'
  declare invalid: boolean
  private initial = ''

  static styles = [
    base,
    css`
      /* 44 pt high, always: the select itself is that high, so a tap anywhere in the box opens the menu; the field is
         drawn 36 pt high behind it */
      :host {
        --ui-hit: 44px;
      }
      :host {
        display: inline-block;
        vertical-align: middle;
        min-width: var(--ui-hit, 0px);
        max-width: 100%;
        height: fit-content;
      }
      .box {
        position: relative;
        display: flex;
        align-items: center;
        width: 100%;
      }
      /* the drawn field */
      .box::before {
        content: '';
        position: absolute;
        inset-inline: 0;
        top: 50%;
        height: 36px;
        transform: translateY(-50%);
        border-radius: var(--radius-md);
        background: var(--fill-tertiary);
        pointer-events: none;
      }
      select {
        position: relative;
        appearance: none;
        display: block;
        width: 100%;
        min-width: 0;
        margin: 0;
        border: 0;
        /* the end clears the chevrons */
        padding-block: 0;
        padding-inline: 12px 30px;
        min-height: max(36px, var(--ui-hit, 0px));
        outline: none; /* the ring is drawn around the field, below */
        background: transparent;
        font: var(--type-body);
        letter-spacing: var(--tracking-body);
        color: var(--label);
        text-overflow: ellipsis;
        cursor: pointer;
      }
      .box:has(select:focus-visible)::before {
        outline: 3px solid var(--focus-ring);
        outline-offset: 2px;
      }
      /* bare, in a row: the ring is drawn tight, so that it fits the row's 44 pt (the section clips what is outside) */
      :host([variant='bare']) .box:has(select:focus-visible)::before {
        outline-offset: 0;
      }
      :host([invalid]) .box:not(:has(select:focus-visible))::before {
        box-shadow: inset 0 0 0 1.5px var(--ios-red);
      }
      /* bare, in a row: no box, the choice in the secondary colour at the row's end, as a value is in an iPadOS form */
      :host([variant='bare']) .box::before,
      :host([variant='bare'][invalid]) .box:not(:has(select:focus-visible))::before {
        background: transparent;
        box-shadow: none;
        inset-inline: -6px;
      }
      /* as wide as the choice it shows (where the browser can: field-sizing), not as its longest option, and never so
         wide that the row's title has no room */
      :host([variant='bare']) {
        max-width: min(100%, 10em);
      }
      :host([variant='bare']) select {
        field-sizing: content;
        padding-inline: 0 18px;
        color: var(--label-secondary);
        text-align: end;
      }
      :host([variant='bare']) svg {
        inset-inline-end: 0;
      }
      :host([variant='bare'][invalid]) select:not(:focus-visible) {
        color: var(--ios-red);
      }
      select:disabled {
        cursor: default;
      }
      .box:has(select:disabled) {
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
    `
  ]

  constructor() {
    super()
    this.options = []
    this.value = ''
    this.required = false
    this.variant = 'field'
    this.invalid = false
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

  /** The select's own `input` crosses to the page, as a field's does: `value` is updated before anyone hears it. */
  private onInput = (e: Event): void => {
    this.value = (e.target as HTMLSelectElement).value
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
        aria-invalid=${ifDefined(this.invalid ? 'true' : undefined)}
        ?required=${this.required}
        @input=${this.onInput}
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
