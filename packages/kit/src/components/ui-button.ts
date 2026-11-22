import { LitElement, css, html } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { inkFor, cssColor } from 'kit:contrast'
import { base } from '../styles'


/**
 * A button in the four iOS styles. It is a real `<button>` inside, so Enter and Space press it, and it submits or
 * resets the form it sits in (`type`).
 *
 * `fill` takes a data colour (a palette token such as `orange`, or `#rrggbb`) for a `filled` button; the text colour
 * is picked for 4.5:1 on it. Unlike `tint`, which is the control's accent, a fill is content.
 *
 * @tag ui-button
 * @slot - The label.
 * @slot icon - A leading icon, for example `<ui-icon slot="icon" name="plus">`.
 * @csspart button - The inner button.
 * @cssprop --tint - The accent of the button; defaults to the inherited `--tint`.
 */
export class UiButton extends LitElement {
  static formAssociated = true
  static shadowRootOptions = { ...LitElement.shadowRootOptions, delegatesFocus: true }
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
    /** Dims the button and ignores presses. */
        disabled: { type: Boolean, reflect: true },
    /** Makes it a toggle button: `aria-pressed` follows `pressed`. */
    toggle: { type: Boolean, reflect: true },
    /** The pressed state of a `toggle` button. */
    pressed: { type: Boolean, reflect: true },
    /** `button` (default), `submit` or `reset`. */
    type: { type: String },
    /** The accessible name, when the visible text is not enough (WCAG 2.5.3: keep the visible text in it). */
    label: { type: String },
    /** Read after the name. */
    description: { type: String },
    /** The form value name, for a submit button. */
    name: { type: String },
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
  declare disabled: boolean
  declare toggle: boolean
  declare pressed: boolean
  declare type: 'button' | 'submit' | 'reset'
  declare label: string
  declare description: string
  declare name: string
  declare value: string
  private internals = this.attachInternals()

  static styles = [
    base,
    css`
      :host {
        display: inline-flex;
        vertical-align: middle;
      }
      :host([size='fill']) {
        display: flex;
        width: 100%;
        height: 100%;
      }
      :host([disabled]) {
        pointer-events: none;
      }
      button {
        --_tint: var(--btn-tint, var(--tint));
        appearance: none;
        border: 0;
        margin: 0;
        width: 100%;
        height: 100%;
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
        min-height: 44px;
      }
      :host([shape='capsule']) button {
        border-radius: 999px;
      }
      :host([shape='circle']) button {
        border-radius: 50%;
      }
      :host([square]) button {
        padding: 0;
        width: 34px;
      }
      :host([square][size='small']) button {
        width: 28px;
      }
      :host([square][size='large']) button {
        width: 50px;
      }

      /* White text on the accent: darkened in dark mode to keep 4.5:1. An explicit fill is used as given. */
      .filled {
        background: var(--btn-fill, light-dark(var(--_tint), color-mix(in oklab, var(--_tint) 70%, black)));
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
        --_ink: light-dark(color-mix(in oklab, var(--_tint) 78%, black), color-mix(in oklab, var(--_tint) 78%, white));
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
        font-weight: 400;
        padding-inline: 4px;
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
    this.disabled = false
    this.toggle = false
    this.pressed = false
    this.type = 'button'
    this.label = ''
    this.description = ''
    this.name = ''
    this.value = ''
  }

  private onClick = (): void => {
    if (this.disabled) return
    const form = this.internals.form
    if (this.type === 'submit') {
      // like a native submit button, it is part of the data of the submission it starts, and of no other
      this.internals.setFormValue(this.name ? this.value : null)
      form?.requestSubmit()
      this.internals.setFormValue(null)
    } else if (this.type === 'reset') form?.reset()
  }

  render() {
    const fill = this.variant === 'filled' && this.fill ? cssColor(this.fill) : undefined
    const ink = fill ? inkFor(this.fill) : undefined
    const tint = this.destructive ? 'var(--ios-red)' : this.tint
    const vars = [
      tint ? `--btn-tint:${tint}` : '',
      fill ? `--btn-fill:${fill}` : tint && !this.destructive ? `--btn-fill:${tint}` : '',
      ink ? `--btn-ink:${ink}` : ''
    ]
      .filter(Boolean)
      .join(';')
    return html`<button
      part="button"
      class="${this.variant} ${fill ? 'inverts' : ''}"
      type="button"
      style=${ifDefined(vars || undefined)}
      ?disabled=${this.disabled}
      aria-pressed=${ifDefined(this.toggle ? String(this.pressed) : undefined)}
      aria-label=${ifDefined(this.label || undefined)}
      aria-description=${ifDefined(this.description || undefined)}
      @click=${this.onClick}
    >
      <slot name="icon"></slot><slot></slot>
    </button>`
  }
}

export type ButtonVariant = UiButton['variant']
export type ButtonSize = UiButton['size']
export type ButtonShape = UiButton['shape']

customElements.define('ui-button', UiButton)
declare global {
  interface HTMLElementTagNameMap {
    'ui-button': UiButton
  }
}
