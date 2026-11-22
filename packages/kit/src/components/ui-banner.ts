import { LitElement, css, html } from 'lit'
import './ui-icon'
import { base } from '../styles'

type BannerTone = 'info' | 'success' | 'warning' | 'error'

const ICONS: Record<BannerTone, string> = {
  info: 'info.circle',
  success: 'checkmark',
  warning: 'exclamationmark.triangle',
  error: 'exclamationmark.triangle'
}
const COLORS: Record<BannerTone, string> = {
  info: 'var(--ios-blue)',
  success: 'var(--ios-green)',
  warning: 'var(--ios-orange)',
  error: 'var(--ios-red)'
}

/**
 * An inline message with an optional action, for something the person should know about in place (a failed save, a
 * missing permission). It is announced when it appears: a `status` region, or an `alert` for an `error`.
 *
 * @tag ui-banner
 * @slot - The message.
 * @slot action - The action, usually `<ui-button slot="action" variant="plain" size="small">`.
 * @csspart banner - The banner's box.
 */
export class UiBanner extends LitElement {
  static properties = {
    /** `info` (default), `success`, `warning` or `error`. */
    tone: { type: String, reflect: true },
    /** A bold first line. */
    heading: { type: String }
  }
  declare tone: 'info' | 'success' | 'warning' | 'error'
  declare heading: string

  static styles = [
    base,
    css`
      :host {
        display: block;
      }
      .banner {
        display: flex;
        align-items: flex-start;
        gap: 10px;
        padding: 10px 12px;
        border-radius: var(--radius-md);
        /* Tinted in light mode; neutral in dark, where a tint under a plain action button costs its 4.5:1. */
        background: light-dark(color-mix(in srgb, var(--_color) 14%, var(--bg-grouped-elevated-secondary)), var(--bg-grouped-elevated-secondary));
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--_color) 35%, transparent);
        font: var(--type-subheadline);
        letter-spacing: var(--tracking-subheadline);
        color: var(--label);
      }
      .icon {
        flex: none;
        display: flex;
        color: var(--_color);
        padding-top: 1px;
      }
      .text {
        flex: 1;
        min-width: 0;
        overflow-wrap: anywhere;
      }
      .heading {
        display: block;
        font-weight: 600;
      }
      .action {
        flex: none;
        align-self: center;
      }
    `
  ]

  constructor() {
    super()
    this.tone = 'info'
    this.heading = ''
  }

  render() {
    const tone: BannerTone = this.tone in ICONS ? this.tone : 'info'
    return html`<div
      class="banner"
      part="banner"
      role=${tone === 'error' ? 'alert' : 'status'}
      style="--_color:${COLORS[tone]}"
    >
      <span class="icon" aria-hidden="true"><ui-icon name=${ICONS[tone]} size="20"></ui-icon></span>
      <span class="text">
        ${this.heading ? html`<span class="heading">${this.heading}</span>` : ''}
        <slot></slot>
      </span>
      <span class="action"><slot name="action"></slot></span>
    </div>`
  }
}

export type { BannerTone }

customElements.define('ui-banner', UiBanner)
declare global {
  interface HTMLElementTagNameMap {
    'ui-banner': UiBanner
  }
}
