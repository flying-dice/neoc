import { LitElement, css, html, type TemplateResult } from 'lit'
import { define } from '../define'
import { base } from '../styles'

/**
 * A summary tile: a small label over one headline value, with room under it for a `ui-meter` or a note. A grid of
 * them is the top of a monitor, like the tiles of the Health summary.
 *
 * @tag ui-tile
 * @slot - The headline value: a big number with its unit.
 * @slot footer - What goes under the value: a `ui-meter`, a note.
 * @csspart tile - The tile's box.
 */
export class UiTile extends LitElement {
  static properties = {
    /** What the tile is about (`CPU`). */
    heading: { type: String }
  }
  declare heading: string

  static styles = [
    base,
    css`
      :host {
        display: block;
        min-width: 0;
      }
      .tile {
        display: flex;
        flex-direction: column;
        gap: 6px;
        min-width: 0;
        padding: 12px 14px;
        border-radius: var(--radius-lg);
        background: var(--bg-grouped-elevated-secondary);
        box-shadow: var(--shadow-card);
        color: var(--label);
      }
      .heading {
        font: var(--type-footnote);
        letter-spacing: var(--tracking-footnote);
        color: var(--label-secondary);
      }
      .value {
        display: flex;
        align-items: baseline;
        flex-wrap: wrap;
        gap: 0 6px;
        min-width: 0;
        font-variant-numeric: tabular-nums;
      }
      ::slotted([slot='footer']) {
        display: block;
      }
    `
  ]

  constructor() {
    super()
    this.heading = ''
  }

  render(): TemplateResult {
    return html`<section class="tile" part="tile" aria-label=${this.heading || null}>
      <div class="heading" aria-hidden="true">${this.heading}</div>
      <div class="value"><slot></slot></div>
      <slot name="footer"></slot>
    </section>`
  }
}

define('ui-tile', UiTile)
declare global {
  interface HTMLElementTagNameMap {
    'ui-tile': UiTile
  }
}
