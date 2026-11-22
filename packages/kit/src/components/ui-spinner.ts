import { LitElement, css, html, svg } from 'lit'
import { base } from '../styles'

/**
 * A busy indicator (UIActivityIndicatorView). It says the app is working, so it keeps turning under Reduce Motion, as
 * iOS's does.
 *
 * @tag ui-spinner
 */
export class UiSpinner extends LitElement {
  static properties = {
    /** `medium` (20 px, default) or `large` (37 px). */
    size: { type: String, reflect: true },
    /** The accessible name. */
    label: { type: String }
  }
  declare size: 'medium' | 'large'
  declare label: string

  static styles = [
    base,
    css`
      :host {
        display: inline-block;
        flex: none;
        vertical-align: middle;
        width: 20px;
        height: 20px;
        color: var(--label-secondary);
      }
      :host([size='large']) {
        width: 37px;
        height: 37px;
      }
      svg {
        display: block;
        width: 100%;
        height: 100%;
        animation: spin 0.8s steps(8) infinite;
      }
      @keyframes spin {
        to {
          transform: rotate(360deg);
        }
      }
      @media (prefers-reduced-motion: reduce) {
        svg {
          animation-duration: 0.8s !important;
        }
      }
    `
  ]

  constructor() {
    super()
    this.size = 'medium'
    this.label = 'Loading'
  }

  render() {
    const ticks = Array.from({ length: 8 }, (_, i) => i)
    return html`<span role="status" aria-label=${this.label}>
      <svg viewBox="0 0 20 20" aria-hidden="true">
        ${ticks.map(
          (i) => svg`<rect x="9" y="1.5" width="2" height="5" rx="1" fill="currentColor" opacity=${0.15 + (i / 8) * 0.85} transform="rotate(${i * 45} 10 10)" />`
        )}
      </svg>
    </span>`
  }
}

customElements.define('ui-spinner', UiSpinner)
declare global {
  interface HTMLElementTagNameMap {
    'ui-spinner': UiSpinner
  }
}
