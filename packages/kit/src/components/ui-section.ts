import { LitElement, css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { base } from '../styles'

let nextId = 0

/**
 * A group of rows with an optional header and footer. It takes the look of the `ui-list` it sits in; alone it is the
 * `insetGrouped` block of iPadOS Settings.
 *
 * A form is a section of rows with a field each, and its footer is where the form speaks: a note on what the fields
 * are for, or, with `footer-tone="error"`, what is wrong with what was typed (red, and announced when it appears).
 *
 * @tag ui-section
 * @slot - The `ui-row`s.
 * @csspart rows - The box around the rows.
 * @csspart footer - The note under the rows.
 */
export class UiSection extends LitElement {
  static properties = {
    /** The title above the rows. */
    header: { type: String },
    /** A note under the rows. */
    footer: { type: String },
    /** `note` (default): the footer explains the rows. `error`: it says what is wrong with them, in red, as an alert. */
    footerTone: { type: String, attribute: 'footer-tone', reflect: true },
    /** The look; set by the list that holds the section. */
    variant: { type: String, reflect: true }
  }
  declare header: string
  declare footer: string
  declare footerTone: 'note' | 'error'
  declare variant: 'insetGrouped' | 'inset' | 'plain' | 'grouped' | 'sidebar'
  private uid = `ui-section-${nextId++}`

  static styles = [
    base,
    css`
      :host {
        display: block;
        margin: 0 0 35px;
        min-width: 0;
      }
      .header {
        margin: 0;
        padding: 0 var(--row-inset) 7px;
        font: var(--type-footnote);
        letter-spacing: var(--tracking-footnote);
        font-weight: 400;
        text-transform: uppercase;
        color: var(--label-secondary);
        overflow-wrap: anywhere;
      }
      .footer {
        margin: 0;
        padding: 7px var(--row-inset) 0;
        font: var(--type-footnote);
        letter-spacing: var(--tracking-footnote);
        color: var(--label-secondary);
      }
      :host([footer-tone='error']) .footer {
        color: var(--ios-red);
      }
      .rows {
        overflow: hidden;
        background: var(--list-row-bg, var(--bg-grouped-elevated-secondary));
      }

      /* insetGrouped (Settings): rounded blocks inset from the edges. */
      :host([variant='insetGrouped']) {
        padding: 0 var(--list-inset, 20px);
      }
      :host([variant='insetGrouped']) .rows {
        border-radius: var(--radius-md);
      }
      /* inset: rounded blocks inset from the edges, on the primary background. */
      :host([variant='inset']) {
        padding: 0 var(--list-inset, 20px);
        margin-bottom: 24px;
        --list-row-bg: var(--bg-secondary);
      }
      :host([variant='inset']) .rows {
        border-radius: var(--radius-md);
      }
      /* plain: edge to edge, on the primary background. */
      :host([variant='plain']) {
        margin-bottom: 0;
        --list-row-bg: var(--bg-elevated);
      }
      :host([variant='plain']) .rows {
        background: var(--bg-elevated);
      }
      :host([variant='plain']) .header {
        padding-top: 12px;
        padding-bottom: 6px;
        background: var(--bg-grouped-elevated);
      }
      /* grouped: edge to edge blocks on the grouped background. */
      /* sidebar: rows float on the grouped background, selection is a filled pill. */
      :host([variant='sidebar']) {
        margin-bottom: 22px;
        padding: 0 10px;
        --list-separator: transparent;
        --list-row-radius: var(--radius-md);
        --list-row-bg: transparent;
      }
      :host([variant='sidebar']) .header {
        padding-inline: 10px;
        font: var(--type-headline);
        text-transform: none;
        color: var(--label);
      }
      :host([variant='sidebar']) .rows {
        background: transparent;
        overflow: visible;
      }
    `
  ]

  constructor() {
    super()
    this.header = ''
    this.footer = ''
    this.footerTone = 'note'
    this.variant = 'insetGrouped'
  }

  connectedCallback(): void {
    super.connectedCallback()
    const list = this.closest<HTMLElement & { variant?: ListVariant }>('ui-list')
    if (list?.variant) this.variant = list.variant
  }

  render(): TemplateResult {
    const selection = this.closest<HTMLElement & { selection?: string }>('ui-list')?.selection
    const selectable = selection === 'single' || selection === 'multi'
    // in a listbox only options may be its children: the header and footer are labels, not content
    const presentation = selectable ? 'presentation' : undefined
    return html`${this.header ? html`<h2 class="header" id=${this.uid} role=${ifDefined(presentation)}>${this.header}</h2>` : ''}
      <div
        class="rows"
        part="rows"
        role=${selectable ? 'group' : 'list'}
        aria-labelledby=${ifDefined(this.header ? this.uid : undefined)}
      >
        <slot></slot>
      </div>
      ${
        this.footer
          ? html`<p
              class="footer"
              part="footer"
              role=${ifDefined(presentation ?? (this.footerTone === 'error' ? 'alert' : undefined))}
            >
              ${this.footer}
            </p>`
          : ''
      }`
  }
}

export type ListVariant = UiSection['variant']

define('ui-section', UiSection)
declare global {
  interface HTMLElementTagNameMap {
    'ui-section': UiSection
  }
}
