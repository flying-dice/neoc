import { css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { FormControl, type ValidityResult } from '../form-control'
import { base } from '../styles'

const TYPES = ['text', 'email', 'password', 'url', 'search', 'tel', 'number']

/**
 * A single-line text field that takes what an ordinary text input takes: `type` (including `number`), `inputmode`,
 * `maxlength`, `minlength`, `pattern`, `min`, `max`, `step`, `enterkeyhint`, `autocapitalize`, `autocomplete`. A form
 * control (see `FormControl`): with a `name` it submits its value, Enter submits the form, and `required`, the length and
 * pattern limits and `setCustomValidity` take part in validation (`checkValidity`, `validity`, ...).
 *
 * The field is drawn 36 pt high and has a 44 pt high hit area (a click on the strip around it focuses it).
 *
 * @tag ui-text-field
 * @fires {InputEvent} input - The person typed. `value` is already updated (the native event, passed through).
 * @fires {Event} change - The person committed the text (blur). It has no `detail`: read `value`.
 * @csspart input - The inner input.
 */
export class UiTextField extends FormControl {
  static properties = {
    /** The text. */
    value: { type: String },
    /** A hint shown while empty. It is not the accessible name. */
    placeholder: { type: String },
    /** `text` (default), `email`, `password`, `url`, `search`, `tel` or `number`. */
    type: { type: String },
    /** The text can be selected and copied, not changed. */
    readonly: { type: Boolean, reflect: true },
    /** The form is invalid while the field is empty. */
    required: { type: Boolean, reflect: true },
    /** The `autocomplete` hint for the browser, for example `email`. */
    autocomplete: { type: String },
    /** The on-screen keyboard to offer: `numeric`, `decimal`, `email`, `tel`, `url`, `search`, `text` or `none`. */
    inputMode: { type: String, attribute: 'inputmode' },
    /** The longest text, in characters. */
    maxlength: { type: Number },
    /** The shortest text, in characters, when there is any. */
    minlength: { type: Number },
    /** A regular expression the whole text must match. */
    pattern: { type: String },
    /** For `type="number"`: the smallest value. */
    min: { type: String },
    /** For `type="number"`: the largest value. */
    max: { type: String },
    /** For `type="number"`: the step between values, or `any`. */
    step: { type: String },
    /** What the keyboard's return key says: `enter`, `done`, `go`, `next`, `previous`, `search` or `send`. */
    enterKeyHint: { type: String, attribute: 'enterkeyhint' },
    /** How the keyboard capitalises: `off`, `none`, `on`, `sentences`, `words` or `characters`. */
    autocapitalize: { type: String },
    /** `field` (default): a rounded filled field. `bare`: no box, for use inside a `ui-row`. */
    variant: { type: String, reflect: true },
    /** Shows the field as invalid (`aria-invalid`) with the message from an error `ui-banner` or the row's footer. */
    invalid: { type: Boolean, reflect: true }
  }
  declare value: string
  declare placeholder: string
  declare type: 'text' | 'email' | 'password' | 'url' | 'search' | 'tel' | 'number'
  declare readonly: boolean
  declare required: boolean
  declare autocomplete: string
  declare inputMode: string
  declare maxlength: number | undefined
  declare minlength: number | undefined
  declare pattern: string | undefined
  declare min: string | undefined
  declare max: string | undefined
  declare step: string | undefined
  declare enterKeyHint: string
  declare autocapitalize: string
  declare variant: 'field' | 'bare'
  declare invalid: boolean
  private initial = ''

  static styles = [
    base,
    css`
      :host {
        display: inline-block;
        vertical-align: middle;
        min-width: 0;
      }
      /* 44 pt high: the strip around the 36 pt field is layout space the field owns (a click on it focuses the input) */
      :host(:not([variant='bare'])) {
        display: inline-flex;
        align-items: center;
        min-height: 44px;
      }
      :host([variant='bare']) {
        display: block;
        flex: 1;
      }
      input {
        display: block;
        width: 100%;
        min-width: 0;
        margin: 0;
        border: 0;
        padding: 0 12px;
        min-height: 36px;
        border-radius: var(--radius-md);
        outline-offset: 2px;
        background: var(--fill-tertiary);
        font: var(--type-body);
        letter-spacing: var(--tracking-body);
        color: var(--label);
        caret-color: var(--tint);
        -webkit-user-select: text;
        user-select: text;
      }
      :host([variant='bare']) input {
        padding: 0;
        min-height: 0;
        border-radius: 0;
        background: transparent;
        outline-offset: 6px;
      }
      :host([invalid]) input:not(:focus-visible) {
        box-shadow: inset 0 0 0 1.5px var(--ios-red);
      }
      input:disabled {
        opacity: 0.4;
      }
      input::placeholder {
        color: var(--placeholder);
      }
      input::-webkit-search-cancel-button {
        display: none;
      }
    `
  ]

  constructor() {
    super()
    this.value = ''
    this.placeholder = ''
    this.type = 'text'
    this.readonly = false
    this.required = false
    this.autocomplete = ''
    this.inputMode = ''
    this.enterKeyHint = ''
    this.autocapitalize = ''
    this.variant = 'field'
    this.invalid = false
  }

  private get input(): HTMLInputElement | null {
    return this.renderRoot.querySelector('input')
  }

  protected get formValue(): string | null {
    return this.value
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

  /** The inner input does the checking (required, length, pattern, type, range); the field reports what it finds. */
  protected computeValidity(): ValidityResult {
    const input = this.input
    if (!input || input.validity.valid) return { flags: {}, message: '' }
    const v = input.validity
    return {
      flags: {
        valueMissing: v.valueMissing,
        typeMismatch: v.typeMismatch,
        patternMismatch: v.patternMismatch,
        tooLong: v.tooLong,
        tooShort: v.tooShort,
        rangeUnderflow: v.rangeUnderflow,
        rangeOverflow: v.rangeOverflow,
        stepMismatch: v.stepMismatch,
        badInput: v.badInput
      },
      message: input.validationMessage
    }
  }

  /** Selects the text, as the native input does. */
  select(): void {
    this.input?.select()
  }

  private onInput = (e: Event): void => {
    this.value = (e.target as HTMLInputElement).value
  }

  private onChange = (): void => {
    // the native `change` does not cross the shadow boundary
    this.dispatchEvent(new Event('change', { bubbles: true, composed: true }))
  }

  private onKeydown = (e: KeyboardEvent): void => {
    if (e.key === 'Enter' && !e.isComposing) this.internals.form?.requestSubmit()
  }

  render(): TemplateResult {
    const type = TYPES.includes(this.type) ? this.type : 'text'
    return html`<input
      part="input"
      .value=${this.value}
      type=${type}
      placeholder=${ifDefined(this.placeholder || undefined)}
      autocomplete=${ifDefined(this.autocomplete || undefined)}
      inputmode=${ifDefined(this.inputMode || undefined)}
      enterkeyhint=${ifDefined(this.enterKeyHint || undefined)}
      autocapitalize=${ifDefined(this.autocapitalize || undefined)}
      maxlength=${ifDefined(this.maxlength)}
      minlength=${ifDefined(this.minlength)}
      pattern=${ifDefined(this.pattern || undefined)}
      min=${ifDefined(this.min)}
      max=${ifDefined(this.max)}
      step=${ifDefined(this.step)}
      aria-label=${ifDefined(this.accessibleName)}
      aria-description=${ifDefined(this.description || undefined)}
      aria-invalid=${ifDefined(this.invalid ? 'true' : undefined)}
      ?disabled=${this.isDisabled}
      ?readonly=${this.readonly}
      ?required=${this.required}
      @input=${this.onInput}
      @change=${this.onChange}
      @keydown=${this.onKeydown}
    />`
  }
}

export type TextFieldType = UiTextField['type']

define('ui-text-field', UiTextField)
declare global {
  interface HTMLElementTagNameMap {
    'ui-text-field': UiTextField
  }
}
