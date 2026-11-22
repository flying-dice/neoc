import { LitElement, css, html } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { base } from '../styles'

/**
 * A single-line text field. A form control: with a `name` it submits its value, Enter submits the form, and
 * `required` takes part in validation.
 *
 * @tag ui-text-field
 * @fires {InputEvent} input - The person typed. `value` is already updated (the native event, passed through).
 * @fires {Event} change - The person committed the text (blur).
 * @csspart input - The inner input.
 */
export class UiTextField extends LitElement {
  static formAssociated = true
  static shadowRootOptions = { ...LitElement.shadowRootOptions, delegatesFocus: true }
  static properties = {
    /** The text. */
        value: { type: String },
    /** A hint shown while empty. It is not the accessible name. */
        placeholder: { type: String },
    /** `text` (default), `email`, `password`, `url`, `search` or `tel`. */
    type: { type: String },
    /** The accessible name; a field needs one (the placeholder is not a name). A `<label for>` names it too. */
    label: { type: String },
    /** Read after the name. */
    description: { type: String },
    /** The form field name. Nothing is submitted without one. */
        name: { type: String },
    /** Dims the field and ignores input. */
        disabled: { type: Boolean, reflect: true },
    /** The text can be selected and copied, not changed. */
        readonly: { type: Boolean, reflect: true },
    /** The form is invalid while the field is empty. */
        required: { type: Boolean, reflect: true },
    /** The `autocomplete` hint for the browser, for example `email`. */
        autocomplete: { type: String },
    /** `field` (default): a rounded filled field. `bare`: no box, for use inside a `ui-row`. */
    variant: { type: String, reflect: true },
    /** Shows the field as invalid (`aria-invalid`) with the message from an error `ui-banner` or the row's footer. */
    invalid: { type: Boolean, reflect: true }
  }
  declare value: string
  declare placeholder: string
  declare type: 'text' | 'email' | 'password' | 'url' | 'search' | 'tel'
  declare label: string
  declare description: string
  declare name: string
  declare disabled: boolean
  declare readonly: boolean
  declare required: boolean
  declare autocomplete: string
  declare variant: 'field' | 'bare'
  declare invalid: boolean
  private internals = this.attachInternals()
  private initial = ''
  private formDisabled = false

  static styles = [
    base,
    css`
      :host {
        display: inline-block;
        vertical-align: middle;
        min-width: 0;
      }
      :host([variant='bare']) {
        display: block;
        flex: 1;
      }
      :host([disabled]) {
        opacity: 0.4;
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
    this.label = ''
    this.description = ''
    this.name = ''
    this.disabled = false
    this.readonly = false
    this.required = false
    this.autocomplete = ''
    this.variant = 'field'
    this.invalid = false
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.initial = this.value
  }

  private get input(): HTMLInputElement | null {
    return this.renderRoot.querySelector('input')
  }

  protected updated(): void {
    this.internals.setFormValue(this.name ? this.value : null)
    const input = this.input
    if (this.required && this.value === '' && input) {
      this.internals.setValidity({ valueMissing: true }, 'Fill out this field.', input)
    } else this.internals.setValidity({})
  }

  formResetCallback(): void {
    this.value = this.initial
  }

  formDisabledCallback(disabled: boolean): void {
    this.formDisabled = disabled
    this.requestUpdate()
  }

  formStateRestoreCallback(state: string | File | FormData | null): void {
    if (typeof state === 'string') this.value = state
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

  render() {
    const name = this.label || this.internals.labels?.[0]?.textContent?.trim() || undefined
    return html`<input
      part="input"
      .value=${this.value}
      type=${this.type}
      placeholder=${ifDefined(this.placeholder || undefined)}
      autocomplete=${ifDefined(this.autocomplete || undefined)}
      aria-label=${ifDefined(name)}
      aria-description=${ifDefined(this.description || undefined)}
      aria-invalid=${ifDefined(this.invalid ? 'true' : undefined)}
      ?disabled=${this.disabled || this.formDisabled}
      ?readonly=${this.readonly}
      ?required=${this.required}
      @input=${this.onInput}
      @change=${this.onChange}
      @keydown=${this.onKeydown}
    />`
  }
}

export type TextFieldType = UiTextField['type']

customElements.define('ui-text-field', UiTextField)
declare global {
  interface HTMLElementTagNameMap {
    'ui-text-field': UiTextField
  }
}
