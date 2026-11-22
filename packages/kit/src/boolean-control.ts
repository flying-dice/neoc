import { LitElement } from 'lit'

/**
 * What `ui-toggle` and `ui-check` share: an on/off value that takes part in a form (`name`, `value`, reset, a
 * disabled fieldset), says so with `change`, and is named by `label` or by a `<label>` that points at it.
 *
 * The control inside is a real `<button role=...>`, so it has focus, Space and Enter for free. A click on a
 * `<label for>` reaches the host, not that button, so the host toggles for a click that did not start inside.
 *
 * @fires {CustomEvent<{ checked: boolean }>} change - The person flipped it. `checked` is already updated.
 */
export abstract class BooleanControl extends LitElement {
  static formAssociated = true
  static shadowRootOptions = { ...LitElement.shadowRootOptions, delegatesFocus: true }
  static properties = {
    /** Whether it is on. */
    checked: { type: Boolean, reflect: true },
    /** Dims the control and ignores input. */
        disabled: { type: Boolean, reflect: true },
    /** The accessible name. A `<label for>` pointing at the element names it too. */
    label: { type: String },
    /** Read after the name. */
    description: { type: String },
    /** The form field name. Nothing is submitted without one. */
    name: { type: String },
    /** The value submitted while on. */
    value: { type: String },
    /** On-state colour: any CSS colour. */
    tint: { type: String }
  }
  declare checked: boolean
  declare disabled: boolean
  declare label: string
  declare description: string
  declare name: string
  declare value: string
  declare tint: string | undefined
  protected internals = this.attachInternals()
  private formDisabled = false
  private initial = false

  constructor() {
    super()
    this.checked = false
    this.disabled = false
    this.label = ''
    this.description = ''
    this.name = ''
    this.value = 'on'
  }

  protected get isDisabled(): boolean {
    return this.disabled || this.formDisabled
  }

  /** The accessible name: `label`, else the text of the first `<label>` that names this element. */
  protected get accessibleName(): string | undefined {
    if (this.label) return this.label
    const text = this.internals.labels?.[0]?.textContent?.trim()
    return text || undefined
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.initial = this.checked
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
    this.dispatchEvent(new CustomEvent('change', { detail: { checked: this.checked }, bubbles: true, composed: true }))
  }

  protected updated(): void {
    this.internals.setFormValue(this.checked && this.name ? this.value : null)
  }

  formResetCallback(): void {
    this.checked = this.initial
  }

  formDisabledCallback(disabled: boolean): void {
    this.formDisabled = disabled
    this.requestUpdate()
  }

  formStateRestoreCallback(state: string | File | FormData | null): void {
    this.checked = state === 'on'
  }
}
