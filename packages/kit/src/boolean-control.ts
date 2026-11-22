import { FormControl, type ValidityResult } from './form-control'

/**
 * What `ui-toggle` and `ui-check` share: an on/off value that takes part in a form (see `FormControl`) and says so with
 * `change`. While on it submits `value`; `required` keeps the form invalid until it is on (the "I agree" check).
 *
 * The control inside is a real `<button role=...>`, so it has focus, Space and Enter for free. A click on a
 * `<label for>` reaches the host, not that button, so the host toggles for a click that did not start inside.
 *
 * @fires {CustomEvent<{ checked: boolean }>} change - The person flipped it. `checked` is already updated.
 */
export abstract class BooleanControl extends FormControl {
  static properties = {
    /** Whether it is on. */
    checked: { type: Boolean, reflect: true },
    /** The value submitted while on. */
    value: { type: String },
    /** The form is invalid until it is on. */
    required: { type: Boolean, reflect: true },
    /** On-state colour: any CSS colour. */
    tint: { type: String }
  }
  declare checked: boolean
  declare value: string
  declare required: boolean
  declare tint: string | undefined
  private initial = false

  constructor() {
    super()
    this.checked = false
    this.value = 'on'
    this.required = false
  }

  protected get formValue(): string | null {
    return this.checked ? this.value : null
  }

  protected captureInitial(): void {
    this.initial = this.checked
  }

  protected resetValue(): void {
    this.checked = this.initial
  }

  protected restoreValue(state: string | File | FormData | null): void {
    // the form kept this control's value while it was on, and nothing while it was off
    this.checked = state !== null
  }

  protected computeValidity(): ValidityResult {
    return this.required && !this.checked
      ? { flags: { valueMissing: true }, message: 'Check this box to continue.' }
      : { flags: {}, message: '' }
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('click', this.hostClick)
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('click', this.hostClick)
  }

  private hostClick = (e: Event): void => {
    if (e.composedPath()[0] === this) this.flip()
  }

  protected flip(): void {
    if (this.isDisabled) return
    this.checked = !this.checked
    this.dispatchEvent(
      new CustomEvent('change', {
        detail: { checked: this.checked },
        bubbles: true,
        composed: true
      })
    )
  }
}
