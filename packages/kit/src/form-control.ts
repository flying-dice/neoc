import { LitElement, type PropertyDeclarations } from 'lit'

/** What a control reports to its form: the flags of `ValidityState` that are set, and the message for the first. */
export interface ValidityResult {
  flags: ValidityStateFlags
  message: string
}

/**
 * What every form-associated control of the kit shares, so that it behaves in a form as its native counterpart does,
 * whether its properties are set as attributes (plain HTML) or as properties (React 19, Svelte 5, Lit):
 *
 * - the entry: `name` reflects to the attribute, which is where the form takes the field's name from, and the control
 *   sets its form value itself on every update;
 * - `disabled`, from the control or from a disabled `<fieldset>`;
 * - validity: the native API (`validity`, `validationMessage`, `willValidate`, `checkValidity`, `reportValidity`,
 *   `setCustomValidity`), the control's own rule (`required`, patterns, ...) from `computeValidity`, and `form`, `labels`;
 * - reset and restore, and the accessible name from `label` or a `<label for>`.
 *
 * A subclass says what its value is (`formValue`), when `required` is unmet (in `computeValidity`), and how to reset and
 * restore it.
 */
export abstract class FormControl extends LitElement {
  static formAssociated = true
  static shadowRootOptions = { ...LitElement.shadowRootOptions, delegatesFocus: true }
  static properties: PropertyDeclarations = {
    /** The form field name. Nothing is submitted without one. */
    name: { type: String, reflect: true },
    /**
     * A 44 x 44 pt hit area around the drawn control, as the shell's `hit` is (off by default: the control is drawn at its
     * own size). Set it wherever nothing else gives the control a 44 pt target; a row, a banner and a toolbar item do.
     */
    hit: { type: Boolean, reflect: true },
    /** Dims the control and ignores input; also set by a disabled `<fieldset>` around it. */
    disabled: { type: Boolean, reflect: true },
    /** The accessible name. A `<label for>` pointing at the element names it too. */
    label: { type: String },
    /** Read after the name. */
    description: { type: String }
  }
  declare name: string
  declare disabled: boolean
  declare hit: boolean
  declare label: string
  declare description: string
  protected internals = this.attachInternals()
  private fieldsetDisabled = false
  private customMessage = ''

  constructor() {
    super()
    this.name = ''
    this.disabled = false
    this.hit = false
    this.label = ''
    this.description = ''
  }

  /** The value the form takes from this control: a string, or null for nothing. */
  protected abstract get formValue(): string | null
  /** Remembers the value the control starts with, for `reset`. Called once, after the first render. */
  protected abstract captureInitial(): void
  /** Puts the control back to the value it started with. */
  protected abstract resetValue(): void
  /** Puts the control back to a value the browser kept (back/forward, autofill). */
  protected abstract restoreValue(state: string | File | FormData | null): void

  /** The control's own rule. The default: nothing to check. */
  protected computeValidity(): ValidityResult {
    return { flags: {}, message: '' }
  }

  /** The element in the shadow root the browser points at when it reports a problem. */
  protected get validationAnchor(): HTMLElement | undefined {
    return this.renderRoot.querySelector<HTMLElement>('button, input') ?? undefined
  }

  /** Whether the control takes no input: itself disabled, or inside a disabled fieldset. */
  protected get isDisabled(): boolean {
    return this.disabled || this.fieldsetDisabled
  }

  /** The accessible name: `label`, else the text of the first `<label>` that names this element. */
  protected get accessibleName(): string | undefined {
    return this.label || this.internals.labels?.[0]?.textContent?.trim() || undefined
  }

  /** The form the control is in, or null. */
  get form(): HTMLFormElement | null {
    return this.internals.form
  }

  /** The `<label>` elements that name the control. */
  get labels(): NodeList {
    return this.internals.labels
  }

  get validity(): ValidityState {
    return this.internals.validity
  }

  get validationMessage(): string {
    return this.internals.validationMessage
  }

  get willValidate(): boolean {
    return this.internals.willValidate
  }

  checkValidity(): boolean {
    return this.internals.checkValidity()
  }

  reportValidity(): boolean {
    return this.internals.reportValidity()
  }

  /** Marks the control invalid with a message of the app's own (`''` clears it), as a native input does. */
  setCustomValidity(message: string): void {
    this.customMessage = message
    this.syncForm()
  }

  protected firstUpdated(): void {
    this.captureInitial()
  }

  protected updated(): void {
    this.syncForm()
  }

  /** Tells the form the current value and validity. */
  protected syncForm(): void {
    this.internals.setFormValue(this.formValue)
    const { flags, message } = this.computeValidity()
    const result: ValidityStateFlags = this.customMessage ? { ...flags, customError: true } : flags
    const text = this.customMessage || message
    if (Object.values(result).some(Boolean))
      this.internals.setValidity(result, text, this.validationAnchor)
    else this.internals.setValidity({})
  }

  formResetCallback(): void {
    this.customMessage = ''
    this.resetValue()
  }

  formDisabledCallback(disabled: boolean): void {
    this.fieldsetDisabled = disabled
    this.requestUpdate()
  }

  formStateRestoreCallback(state: string | File | FormData | null): void {
    this.restoreValue(state)
  }
}
