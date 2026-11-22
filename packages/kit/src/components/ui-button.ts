import { css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { inkFor, cssColor } from 'kit:contrast'
import { define } from '../define'
import { FormControl } from '../form-control'
import { base } from '../styles'
import { colourVars } from '../style'

const VARIANTS = ['filled', 'tinted', 'gray', 'plain']

/**
 * A button in the four iOS styles. It is a real `<button>` inside, so Enter and Space press it, and it submits or
 * resets the form it sits in (`type`). A form control like the others (see `FormControl`): a disabled fieldset disables
 * it. A native submit button is the `submitter` of the `submit` event; a kit button cannot be, so `event.submitter` is
 * null and the form's data carries the button's `name` and `value` instead.
 *
 * `fill` takes a data colour (a palette token such as `orange`, or `#rrggbb`) for a `filled` button; the text colour
 * is picked for 4.5:1 on it. Unlike `tint`, which is the control's accent, a fill is content.
 *
 * Drawn 28 or 34 pt high. With `hit` it has a 44 x 44 pt hit area, as the shell's `hit` does (see `FormControl`).
 *
 * @tag ui-button
 * @slot - The label.
 * @slot icon - A leading icon, for example `<ui-icon slot="icon" name="plus">`.
 * @csspart button - The inner button.
 * @cssprop --tint - The accent of the button; defaults to the inherited `--tint`.
 */
export class UiButton extends FormControl {
  static properties = {
    /** `filled` (default), `tinted`, `gray` or `plain`. */
    variant: { type: String, reflect: true },
    /** `small`, `medium` (default), `large`, or `fill`, which takes the whole box the parent gives it. */
    size: { type: String, reflect: true },
    /** `rounded` (default), `capsule` or `circle` (a round key: use with `size="fill"`). */
    shape: { type: String, reflect: true },
    /** A square button sized for one glyph. */
    square: { type: Boolean, reflect: true },
    /** Any CSS colour for the accent; defaults to the inherited `--tint`. */
    tint: { type: String },
    /** A data colour for a `filled` button: a palette token or `#rrggbb`. */
    fill: { type: String },
    /** Red, for an action that deletes or cannot be undone. */
    destructive: { type: Boolean, reflect: true },
    /** Makes it a toggle button: `aria-pressed` follows `pressed`. */
    toggle: { type: Boolean, reflect: true },
    /** The pressed state of a `toggle` button. */
    pressed: { type: Boolean, reflect: true },
    /** `button` (default), `submit` or `reset`. */
    type: { type: String },
    /** The value submitted with `name` by a submit button. */
    value: { type: String }
  }
  declare variant: 'filled' | 'tinted' | 'gray' | 'plain'
  declare size: 'small' | 'medium' | 'large' | 'fill'
  declare shape: 'rounded' | 'capsule' | 'circle'
  declare square: boolean
  declare tint: string | undefined
  declare fill: string | undefined
  declare destructive: boolean
  declare toggle: boolean
  declare pressed: boolean
  declare type: 'button' | 'submit' | 'reset'
  declare value: string

  static styles = [
    base,
    css`
      /* With hit the element is a 44 x 44 pt box with the drawn button centred in it: the hit area is layout space the
         button owns, as the shell's hit is, so it never covers another control's drawn box. Without it the button is
         drawn at its own size. A row or a banner sets --ui-hit on the controls in it. */
      :host([hit]) {
        --ui-hit: 44px;
      }
      :host {
        display: inline-flex;
        align-items: center;
        justify-content: center;
        vertical-align: middle;
        min-width: var(--ui-hit, 0px);
        min-height: var(--ui-hit, 0px);
        pointer-events: none; /* only the drawn button (or its hit box) takes a click, not an empty part of a stretched element */
      }
      :host([size='fill']) {
        display: flex;
        width: 100%;
        height: 100%;
        min-width: 0;
        min-height: 0;
      }
      :host([disabled]) {
        pointer-events: none;
      }
      button {
        --_tint: var(--btn-tint, var(--tint));
        pointer-events: auto;
        position: relative; /* the hit area below */
        appearance: none;
        border: 0;
        margin: 0;
        flex: 1 1 auto; /* fills the element, as the shell's button fills its grid cell or column */
        display: inline-flex;
        align-items: center;
        justify-content: center;
        gap: 6px;
        cursor: pointer;
        white-space: nowrap;
        font: var(--type-subheadline);
        font-weight: 600;
        letter-spacing: var(--tracking-subheadline);
        min-height: 34px;
        padding: 0 14px;
        border-radius: 8px;
        transition:
          opacity var(--duration-fast) ease,
          background-color var(--duration-fast) ease;
      }
      button:active:not(:disabled) {
        opacity: 0.6;
      }
      button:disabled {
        cursor: default;
        opacity: 0.35;
      }
      /* the hit area: the drawn button's own box, and the empty space the element reserves around it */
      button::after {
        content: '';
        position: absolute;
        left: 50%;
        top: 50%;
        width: max(100%, var(--ui-hit, 0px));
        height: max(100%, var(--ui-hit, 0px));
        transform: translate(-50%, -50%);
      }
      :host([size='small']) button {
        min-height: 28px;
        padding: 0 12px;
        border-radius: 7px;
      }
      :host([size='large']) button,
      :host([size='fill']) button {
        font: var(--type-body);
        font-weight: 600;
        letter-spacing: var(--tracking-body);
        min-height: 50px;
        padding: 0 20px;
        border-radius: 12px;
      }
      :host([size='fill']) button {
        width: 100%;
        height: 100%;
        min-height: 44px;
      }
      :host([shape='capsule']) button {
        border-radius: 999px;
      }
      :host([shape='circle']) button {
        border-radius: 50%;
      }
      :host([square]) button {
        flex: none;
        padding: 0;
        width: 34px;
      }
      /* with hit the box is at least 44 wide, but the drawn button keeps its own width */
      :host([hit]) button {
        flex: none;
      }
      :host([square][size='small']) button {
        width: 28px;
      }
      :host([square][size='large']) button {
        width: 50px;
      }
      /* Plain is a text link in the tint: regular weight and 4 pt of padding at every size (declared last, as in the shell). */
      :host([variant='plain']) button {
        font-weight: 400;
        padding-inline: 4px;
      }

      /* White text on the accent: darkened in dark mode to keep 4.5:1. An explicit fill is used as given. */
      .filled {
        background: var(
          --btn-fill,
          light-dark(var(--_tint), color-mix(in oklab, var(--_tint) 70%, black))
        );
        color: var(--btn-ink, #fff);
      }
      /* A pressed data-colour button swaps its fill and its text and gains a ring in the fill colour. */
      .filled.inverts[aria-pressed='true'] {
        background: var(--btn-ink);
        color: var(--btn-fill);
        outline: 2px solid var(--btn-fill);
        outline-offset: -2px;
      }
      /* Tint-coloured text on a tinted or gray fill: darker in light mode, lighter in dark, for 4.5:1. */
      .tinted,
      .gray {
        --_ink: light-dark(
          color-mix(in oklab, var(--_tint) 78%, black),
          color-mix(in oklab, var(--_tint) 78%, white)
        );
      }
      .tinted {
        background: color-mix(in srgb, var(--_tint) 15%, transparent);
        color: var(--_ink);
      }
      .gray {
        background: var(--fill-tertiary);
        color: var(--_ink);
      }
      .plain {
        background: transparent;
        color: var(--_tint);
      }
      ::slotted([slot='icon']) {
        flex: none;
      }
    `
  ]

  constructor() {
    super()
    this.variant = 'filled'
    this.size = 'medium'
    this.shape = 'rounded'
    this.square = false
    this.destructive = false
    this.toggle = false
    this.pressed = false
    this.type = 'button'
    this.value = ''
  }

  protected get formValue(): string | null {
    return null // only the submitting button is part of a submission, and only for that one
  }

  protected captureInitial(): void {
    // a button has no value of its own to reset or restore
  }

  protected resetValue(): void {
    // nothing to reset
  }

  protected restoreValue(): void {
    // nothing to restore
  }

  /** Runs the button's action (a submit, a reset): from the drawn button, and from a click on the element itself. */
  private activate(): void {
    if (this.isDisabled) return
    const form = this.internals.form
    if (this.type === 'submit') {
      // like a native submit button, it is part of the data of the submission it starts, and of no other
      this.internals.setFormValue(this.name ? this.value : null)
      form?.requestSubmit()
      this.internals.setFormValue(null)
    } else if (this.type === 'reset') form?.reset()
  }

  /** A click whose first target is the element (`el.click()`, `fireEvent.click`, a label) acts as a click on the button. */
  private onHostClick = (e: Event): void => {
    if (e.composedPath()[0] === this) this.activate()
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('click', this.onHostClick)
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('click', this.onHostClick)
  }

  render(): TemplateResult {
    const variant = VARIANTS.includes(this.variant) ? this.variant : 'filled'
    const fill = variant === 'filled' && this.fill ? cssColor(this.fill) : undefined
    const ink = fill ? inkFor(this.fill) : undefined
    const tint = this.destructive ? 'var(--ios-red)' : this.tint
    return html`<button
      part="button"
      class="${variant} ${fill ? 'inverts' : ''}"
      type="button"
      style=${colourVars({
        '--btn-tint': tint,
        '--btn-fill': fill ?? (tint && !this.destructive ? tint : undefined),
        '--btn-ink': ink
      })}
      ?disabled=${this.isDisabled}
      aria-pressed=${ifDefined(this.toggle ? String(this.pressed) : undefined)}
      aria-label=${ifDefined(this.label || undefined)}
      aria-description=${ifDefined(this.description || undefined)}
      @click=${() => this.activate()}
    >
      <slot name="icon"></slot><slot></slot>
    </button>`
  }
}

export type ButtonVariant = UiButton['variant']
export type ButtonSize = UiButton['size']
export type ButtonShape = UiButton['shape']

define('ui-button', UiButton)
declare global {
  interface HTMLElementTagNameMap {
    'ui-button': UiButton
  }
}
