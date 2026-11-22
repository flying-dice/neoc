import { LitElement, css, html } from 'lit'
import './ui-icon'
import { base } from '../styles'

/**
 * What a screen shows when there is nothing to show: a muted glyph, a title, an optional message and one call to
 * action.
 *
 * @tag ui-empty-state
 * @slot - Extra content under the message.
 * @slot action - The one call to action, usually a `ui-button`.
 */
export class UiEmptyState extends LitElement {
  static properties = {
    /** The glyph's name from the icon set. */
    icon: { type: String },
    /** The title. */
        heading: { type: String },
    /** A line of explanation under the title. */
        message: { type: String }
  }
  declare icon: string
  declare heading: string
  declare message: string

  static styles = [
    base,
    css`
      :host {
        display: block;
      }
      section {
        display: flex;
        flex-direction: column;
        align-items: center;
        justify-content: center;
        gap: 6px;
        padding: 32px var(--row-inset);
        text-align: center;
        color: var(--label-secondary);
        min-width: 0;
      }
      .glyph {
        display: flex;
        color: var(--label-tertiary);
        margin-bottom: 6px;
      }
      h2 {
        margin: 0;
        font: var(--type-title3);
        font-weight: 600;
        letter-spacing: var(--tracking-title3);
        color: var(--label);
        overflow-wrap: anywhere;
      }
      p {
        margin: 0;
        max-width: 32ch;
        font: var(--type-subheadline);
        letter-spacing: var(--tracking-subheadline);
        overflow-wrap: anywhere;
      }
      .action {
        margin-top: 12px;
      }
      .action:not(:has(*)) {
        display: none;
      }
    `
  ]

  constructor() {
    super()
    this.icon = ''
    this.heading = ''
    this.message = ''
  }

  render() {
    return html`<section aria-label=${this.heading}>
      ${this.icon
        ? html`<span class="glyph" aria-hidden="true"><ui-icon name=${this.icon} size="44" weight="1.6"></ui-icon></span>`
        : ''}
      <h2>${this.heading}</h2>
      ${this.message ? html`<p>${this.message}</p>` : ''}
      <slot></slot>
      <div class="action"><slot name="action"></slot></div>
    </section>`
  }
}

customElements.define('ui-empty-state', UiEmptyState)
declare global {
  interface HTMLElementTagNameMap {
    'ui-empty-state': UiEmptyState
  }
}
