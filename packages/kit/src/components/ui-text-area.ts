import { css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { FormControl, type ValidityResult } from '../form-control'
import { base } from '../styles'

/**
 * A multi-line text field (UITextView, SwiftUI's `TextEditor`): what `ui-text-field` is for one line, for many. Return
 * starts a new line and never submits the form. A form control (see `FormControl`): with a `name` it submits its text,
 * and `required`, the length limits and `setCustomValidity` take part in validation.
 *
 * It is `rows` lines high and scrolls in itself past that; given a height by its parent (a flex or grid item, a
 * `height`), it fills it. There is no resize handle, as there is none on iPadOS.
 *
 * @tag ui-text-area
 * @fires {InputEvent} input - The person typed. `value` is already updated (the native event, passed through).
 * @fires {Event} change - The person committed the text (blur). It has no `detail`: read `value`.
 * @csspart input - The inner textarea: set its `font` for text that is not prose (code, Markdown).
 */
export class UiTextArea extends FormControl {
  static properties = {
    /** The text. */
    value: { type: String },
    /** A hint shown while empty. It is not the accessible name. */
    placeholder: { type: String },
    /** How many lines high it is when its parent gives it no height. */
    rows: { type: Number },
    /** The text can be selected and copied, not changed. */
    readonly: { type: Boolean, reflect: true },
    /** The form is invalid while the field is empty. */
    required: { type: Boolean, reflect: true },
    /** The longest text, in characters. */
    maxlength: { type: Number },
    /** The shortest text, in characters, when there is any. */
    minlength: { type: Number },
    /** What the keyboard's return key says: `enter`, `done`, `go`, `next`, `previous`, `search` or `send`. */
    enterKeyHint: { type: String, attribute: 'enterkeyhint' },
    /** How the keyboard capitalises: `off`, `none`, `on`, `sentences`, `words` or `characters`. */
    autocapitalize: { type: String },
    /** `field` (default): a rounded filled field. `bare`: no box, for text that is the whole pane. */
    variant: { type: String, reflect: true },
    /** Shows the field as invalid (`aria-invalid`): a red outline, or red text for a `bare` field. */
    invalid: { type: Boolean, reflect: true }
  }
  declare value: string
  declare placeholder: string
  declare rows: number
  declare readonly: boolean
  declare required: boolean
  declare maxlength: number | undefined
  declare minlength: number | undefined
  declare enterKeyHint: string
  declare autocapitalize: string
  declare variant: 'field' | 'bare'
  declare invalid: boolean
  private initial = ''

  static styles = [
    base,
    css`
      :host {
        display: block;
        min-width: 0;
        min-height: 0;
      }
      textarea {
        display: block;
        width: 100%;
        height: 100%;
        min-width: 0;
        margin: 0;
        border: 0;
        padding: 8px 12px;
        border-radius: var(--radius-md);
        outline-offset: 2px;
        resize: none;
        background: var(--fill-tertiary);
        font: var(--type-body);
        letter-spacing: var(--tracking-body);
        color: var(--label);
        caret-color: var(--tint);
        -webkit-user-select: text;
        user-select: text;
      }
      /* no box to stand out from: the ring is drawn inside the pane's edge, where nothing clips it */
      :host([variant='bare']) textarea {
        padding: 0;
        border-radius: 0;
        background: transparent;
        outline-offset: -3px;
      }
      :host([invalid]) textarea:not(:focus-visible) {
        box-shadow: inset 0 0 0 1.5px var(--ios-red);
      }
      :host([variant='bare'][invalid]) textarea:not(:focus-visible) {
        box-shadow: none;
        color: var(--ios-red);
      }
      textarea:disabled {
        opacity: 0.4;
      }
      textarea::placeholder {
        color: var(--placeholder);
      }
    `
  ]

  constructor() {
    super()
    this.value = ''
    this.placeholder = ''
    this.rows = 3
    this.readonly = false
    this.required = false
    this.enterKeyHint = ''
    this.autocapitalize = ''
    this.variant = 'field'
    this.invalid = false
  }

  private get input(): HTMLTextAreaElement | null {
    return this.renderRoot.querySelector('textarea')
  }

  protected get validationAnchor(): HTMLElement | undefined {
    return this.input ?? undefined
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

  /** The inner textarea does the checking (required, length); the field reports what it finds. */
  protected computeValidity(): ValidityResult {
    const input = this.input
    if (!input || input.validity.valid) return { flags: {}, message: '' }
    const v = input.validity
    return {
      flags: { valueMissing: v.valueMissing, tooLong: v.tooLong, tooShort: v.tooShort },
      message: input.validationMessage
    }
  }

  /** Selects the text, as the native textarea does. */
  select(): void {
    this.input?.select()
  }

  private onInput = (e: Event): void => {
    this.value = (e.target as HTMLTextAreaElement).value
  }

  private onChange = (): void => {
    // the native `change` does not cross the shadow boundary
    this.dispatchEvent(new Event('change', { bubbles: true, composed: true }))
  }

  render(): TemplateResult {
    return html`<textarea
      part="input"
      .value=${this.value}
      rows=${Number.isInteger(this.rows) && this.rows > 0 ? this.rows : 3}
      placeholder=${ifDefined(this.placeholder || undefined)}
      enterkeyhint=${ifDefined(this.enterKeyHint || undefined)}
      autocapitalize=${ifDefined(this.autocapitalize || undefined)}
      maxlength=${ifDefined(this.maxlength)}
      minlength=${ifDefined(this.minlength)}
      aria-label=${ifDefined(this.accessibleName)}
      aria-description=${ifDefined(this.description || undefined)}
      aria-invalid=${ifDefined(this.invalid ? 'true' : undefined)}
      ?disabled=${this.isDisabled}
      ?readonly=${this.readonly}
      ?required=${this.required}
      @input=${this.onInput}
      @change=${this.onChange}
    ></textarea>`
  }
}

define('ui-text-area', UiTextArea)
declare global {
  interface HTMLElementTagNameMap {
    'ui-text-area': UiTextArea
  }
}
