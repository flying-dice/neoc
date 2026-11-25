import { LitElement, css, html, type TemplateResult } from 'lit'
import { styleMap } from 'lit/directives/style-map.js'
import { define } from '../define'
import { cssColor } from 'kit:contrast'
import './ui-icon'
import { base } from '../styles'

/**
 * One row of a `ui-list` (UITableViewCell): a title and subtitle, an optional icon tile, a value, a chevron, and
 * slots for anything else.
 *
 * A row is plain text until it is `tappable`. A tappable row has one tab stop and fires `press` when it is tapped; the
 * content in its `trailing` slot (a `ui-toggle`, a button) stays separately clickable and does not press the row. In a
 * list with `selection`, a row is an option: it shows `selected`, and a tap, Enter or Space fires `press`.
 *
 * Listen for `press`, not `click`: a `click` bubbles from everything inside the row, a flipped trailing toggle included.
 *
 * @tag ui-row
 * @slot - Custom content in place of `heading` and `subheading`.
 * @slot leading - Content before the text, in place of `icon`.
 * @slot trailing - Content after the text, for example a `ui-toggle`.
 * @csspart face - The row's box.
 * @fires {CustomEvent<void>} press - The person tapped a `tappable` row, or tapped or pressed Enter or Space on an option.
 */
export class UiRow extends LitElement {
  static properties = {
    /** The row's title. (Not `title`: that attribute is the native tooltip.) */
    heading: { type: String },
    /** A second line under the title. */
    subheading: { type: String },
    /** A secondary value at the right, for example `On` or a network name. */
    value: { type: String },
    /** The icon tile's glyph from the icon set. */
    icon: { type: String },
    /** The icon tile's colour: a palette token or `#rrggbb`. Defaults to gray. */
    iconColor: { type: String, attribute: 'icon-color' },
    /** Shows a disclosure chevron at the end. */
    chevron: { type: Boolean, reflect: true },
    /** The row is a button. */
    tappable: { type: Boolean, reflect: true },
    /** Selected (in a list with `selection`). */
    selected: { type: Boolean, reflect: true },
    /** Red title, for a row that deletes. */
    destructive: { type: Boolean, reflect: true },
    /** An accent-coloured action row (`Add Account…`). */
    action: { type: Boolean, reflect: true },
    /** Dims the row and ignores taps. */
    disabled: { type: Boolean, reflect: true },
    /** The accessible name when it should differ from the title and subtitle. */
    label: { type: String }
  }
  declare heading: string
  declare subheading: string
  declare value: string
  declare icon: string
  declare iconColor: string
  declare chevron: boolean
  declare tappable: boolean
  declare selected: boolean
  declare destructive: boolean
  declare action: boolean
  declare disabled: boolean
  declare label: string
  /** Set by the owning list: whether this row is its one tab stop. */
  tabStop = false

  static styles = [
    base,
    css`
      :host {
        display: block;
        position: relative;
      }
      /* dim the content, not the row's own background */
      :host([disabled]) .leading,
      :host([disabled]) .content {
        opacity: 0.4;
      }
      :host(:focus-visible) {
        outline: none;
      }
      .face {
        position: relative;
        display: flex;
        align-items: center;
        gap: 12px;
        min-height: var(--row-height);
        padding-left: var(--row-inset);
        background: var(--list-row-bg, var(--bg-elevated));
        border-radius: var(--list-row-radius, 0);
        color: var(--label);
        font: var(--type-body);
        letter-spacing: var(--tracking-body);
      }
      :host(:focus-visible) .face,
      .hit:focus-visible {
        outline: 3px solid var(--focus-ring);
        outline-offset: -2px;
      }
      .face:has(.hit:active),
      :host([role='option']:not([selected]):active) .face {
        background: var(--fill-secondary);
      }
      :host([role='option']) {
        cursor: pointer;
      }
      :host([selected]) .face {
        background: light-dark(var(--tint), color-mix(in oklab, var(--tint) 70%, black));
        color: #fff;
      }
      /* The ring is drawn on the tint fill, where system blue would vanish: --label holds 3:1 on it in both themes. */
      :host([selected]:not([multi])) .hit:focus-visible,
      :host([selected]:not([multi]):focus-visible) .face {
        outline-color: var(--label);
      }
      /* Multiple selection marks rows with a check circle and leaves the row alone. */
      :host([multi][selected]) .face {
        background: var(--list-row-bg, var(--bg-elevated));
        color: var(--label);
      }
      .check {
        flex: none;
        width: 22px;
        height: 22px;
        margin-right: -2px;
        border-radius: 50%;
        border: 1.5px solid var(--label-tertiary);
        display: flex;
        align-items: center;
        justify-content: center;
        color: #fff;
      }
      .check.on {
        border-color: transparent;
        background: var(--tint-fill);
      }
      .leading {
        flex: none;
        display: flex;
        align-items: center;
        gap: 8px;
        position: relative;
        z-index: 2; /* above the tap overlay, as the trailing content is: a leading check or icon button stays clickable */
        pointer-events: none;
      }
      .leading.none {
        display: none;
      }
      .tile {
        flex: none;
        display: grid;
        place-items: center;
        width: 29px;
        height: 29px;
        border-radius: 6px;
        color: #fff;
        background: var(--ios-gray);
      }
      .content {
        flex: 1;
        min-width: 0;
        align-self: stretch;
        display: flex;
        align-items: center;
        gap: 8px;
        padding: 11px var(--row-inset) 11px 0;
        position: relative;
      }
      /* Inset hairline separator: starts at the text, not the icon. */
      .content::after {
        content: '';
        position: absolute;
        left: 0;
        right: 0;
        bottom: calc(var(--hairline) * -1);
        height: var(--hairline);
        background: var(--list-separator, var(--separator));
      }
      :host(:last-child) .content::after,
      :host([selected]:not([multi])) .content::after {
        display: none;
      }
      .text {
        flex: 1;
        min-width: 0;
        display: flex;
        flex-direction: column;
        gap: 1px;
      }
      .body {
        flex: 1;
        min-width: 0;
        display: flex;
      }
      .body ::slotted(*) {
        position: relative;
        z-index: 2;
      }
      .title,
      .subtitle {
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      :host([destructive]) .title {
        color: var(--ios-red);
      }
      :host([action]) .title {
        color: var(--tint);
      }
      :host([selected]:not([multi])) .title {
        color: #fff;
      }
      /* what a row holds in its slots (a value pair) reads on the selection's fill too */
      :host([selected]:not([multi])) {
        --ui-value-main: #fff;
        --ui-value-secondary: rgb(255 255 255 / 0.92);
      }
      .subtitle {
        font: var(--type-subheadline);
        letter-spacing: var(--tracking-subheadline);
        color: var(--label-secondary);
      }
      :host([selected]:not([multi])) .subtitle,
      :host([selected]:not([multi])) .value,
      :host([selected]:not([multi])) .chevron {
        color: rgb(255 255 255 / 0.92);
      }
      .accessory {
        flex: none;
        display: flex;
        align-items: center;
        gap: 8px;
        position: relative;
        z-index: 2; /* above the tap overlay: interactive trailing content stays clickable */
        pointer-events: none;
      }
      ::slotted(*) {
        pointer-events: auto;
      }
      /* A row gives the kit controls in it the 44 pt box (--ui-hit, the shell's hit), and takes the extra back from the
         empty space it has so that each control sits at the size it is drawn and the row keeps the height and the text
         position it has with the shell's controls. Vertically that is the row's padding. Horizontally, between two
         controls, at most half of the 8 px gap each (so their boxes meet and never overlap); the whole extra only on an
         outer edge, where the neighbour is the row's padding or its text, not a control. */
      ::slotted(
        :is(ui-button, ui-toggle, ui-check, ui-segmented, ui-text-field, ui-text-area, ui-picker)
      ) {
        --ui-hit: 44px;
      }
      ::slotted(ui-toggle) {
        margin-block: -6.5px;
      }
      ::slotted(ui-segmented) {
        margin-block: -6px;
      }
      ::slotted(:is(ui-text-field, ui-picker)) {
        margin-block: -4px;
      }
      /* a bare field has no box to take back: its text sits on the row's own line, level with the label before it */
      ::slotted(ui-text-field[variant='bare']) {
        margin-block: 0;
        align-self: center;
      }
      /* a bare picker has no box either: it takes the row's whole padding, so the row keeps its height and all of it
         opens the menu */
      ::slotted(ui-picker[variant='bare']) {
        margin-block: -11px;
      }
      /* a text area in a row is as high as its lines: it takes nothing back */
      ::slotted(ui-text-area) {
        flex: 1;
      }
      ::slotted(ui-button) {
        margin-block: -5px; /* drawn 34 pt high */
      }
      ::slotted(ui-button[size='small']) {
        margin-block: -8px;
      }
      ::slotted(ui-button[size='large']) {
        margin-block: 0;
      }
      ::slotted(ui-check) {
        margin: -10px -4px;
      }
      ::slotted(ui-check:first-child) {
        margin-inline-start: -10px;
      }
      ::slotted(ui-check:last-child) {
        margin-inline-end: -10px;
      }
      ::slotted(ui-button[square]) {
        margin-inline: -4px;
      }
      ::slotted(ui-button[square]:first-child) {
        margin-inline-start: -5px;
      }
      ::slotted(ui-button[square]:last-child) {
        margin-inline-end: -5px;
      }
      ::slotted(ui-button[square][size='small']:first-child) {
        margin-inline-start: -8px;
      }
      ::slotted(ui-button[square][size='small']:last-child) {
        margin-inline-end: -8px;
      }
      .value {
        color: var(--label-secondary);
      }
      .chevron {
        display: flex;
        color: var(--label-tertiary);
      }
      .hit {
        position: absolute;
        inset: 0;
        z-index: 1;
        appearance: none;
        border: 0;
        padding: 0;
        margin: 0;
        background: transparent;
        cursor: pointer;
        border-radius: inherit;
      }
    `
  ]

  constructor() {
    super()
    this.heading = ''
    this.subheading = ''
    this.value = ''
    this.icon = ''
    this.iconColor = ''
    this.chevron = false
    this.tappable = false
    this.selected = false
    this.destructive = false
    this.action = false
    this.disabled = false
    this.label = ''
  }

  private get list(): (HTMLElement & { selection?: string }) | null {
    return this.closest('ui-list')
  }

  private get option(): boolean {
    const s = this.list?.selection
    return s === 'single' || s === 'multi'
  }

  /** Whether the row takes keyboard focus: an option, or a tappable row. */
  get focusable(): boolean {
    return !this.disabled && (this.option || this.tappable)
  }

  /** Moves focus to the row's focus target. */
  focusRow(): void {
    if (this.option) this.focus()
    else this.renderRoot.querySelector<HTMLElement>('.hit')?.focus()
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('keydown', this.onKeydown)
    this.addEventListener('click', this.onHostClick, true)
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.removeEventListener('keydown', this.onKeydown)
    this.removeEventListener('click', this.onHostClick, true)
  }

  /** A disabled row does not tap; an option presses on a click that is its own, not on one from slotted content. */
  private onHostClick = (e: Event): void => {
    if (this.disabled) {
      e.stopImmediatePropagation()
      e.preventDefault()
    } else if (this.option && e.target === this) this.press()
    // `el.click()` and a click dispatched on the element press a tappable row, as a tap on its button does
    else if (this.tappable && e.composedPath()[0] === this) this.press()
  }

  private press(): void {
    this.dispatchEvent(new CustomEvent('press', { bubbles: true, composed: true }))
  }

  private onKeydown = (e: KeyboardEvent): void => {
    if (!this.option || this.disabled || e.target !== this) return
    if (e.key === 'Enter' || e.key === ' ') {
      e.preventDefault()
      this.click()
    }
  }

  protected willUpdate(): void {
    // the role and the focus target follow the list's selection mode
    const option = this.option
    this.setAttribute('role', option ? 'option' : 'listitem')
    if (this.list?.selection === 'multi') this.setAttribute('multi', '')
    else this.removeAttribute('multi')
    if (option) {
      this.setAttribute('aria-selected', String(this.selected))
      // a disabled option cannot take focus at all (tabindex -1 would still let a script focus it)
      if (this.disabled) this.removeAttribute('tabindex')
      else this.setAttribute('tabindex', this.tabStop ? '0' : '-1')
      const name = this.label || this.accessibleText
      if (name) this.setAttribute('aria-label', name)
      else this.removeAttribute('aria-label') // custom content names the option
    } else {
      this.removeAttribute('aria-selected')
      this.removeAttribute('tabindex')
      this.removeAttribute('aria-label')
    }
    if (this.disabled) this.setAttribute('aria-disabled', 'true')
    else this.removeAttribute('aria-disabled')
  }

  private get accessibleText(): string {
    return this.subheading ? `${this.heading}, ${this.subheading}` : this.heading
  }

  render(): TemplateResult {
    const option = this.option
    const tile = this.icon ? cssColor(this.iconColor || 'gray') : undefined
    const hasLeadingSlot = this.querySelector(':scope > [slot="leading"]') !== null
    return html`<div class="face" part="face">
      ${
        this.list?.selection === 'multi'
          ? html`<span class="check ${this.selected ? 'on' : ''}" aria-hidden="true">
              ${this.selected ? html`<ui-icon name="checkmark" size="13" weight="3"></ui-icon>` : ''}
            </span>`
          : ''
      }
      <span class="leading ${hasLeadingSlot || this.icon ? '' : 'none'}"
        ><slot name="leading" @slotchange=${() => this.requestUpdate()}
          >${
            this.icon
              ? html`<span class="tile" style=${styleMap({ background: tile ?? '' })}
                  ><ui-icon name=${this.icon} size="18" weight="2.2"></ui-icon
                ></span>`
              : ''
          }</slot
        ></span
      >
      <span class="content">
        <span class="body"
          ><slot
            ><span class="text" aria-hidden=${this.tappable && !option ? 'true' : 'false'}>
              <span class="title">${this.heading}</span>
              ${this.subheading ? html`<span class="subtitle">${this.subheading}</span>` : ''}
            </span></slot
          ></span
        >
        <span class="accessory">
          ${this.value ? html`<span class="value">${this.value}</span>` : ''}
          <slot name="trailing"></slot>
          ${
            this.chevron
              ? html`<span class="chevron" aria-hidden="true"
                  ><ui-icon name="chevron.right" size="14" weight="2.6"></ui-icon
                ></span>`
              : ''
          }
        </span>
      </span>
      ${
        this.tappable && !option && !this.disabled
          ? html`<button
              class="hit"
              type="button"
              tabindex=${this.tabStop ? '0' : '-1'}
              aria-label=${this.label || this.accessibleText}
              @click=${() => this.press()}
            ></button>`
          : ''
      }
    </div>`
  }
}

define('ui-row', UiRow)
declare global {
  interface HTMLElementTagNameMap {
    'ui-row': UiRow
  }
}
