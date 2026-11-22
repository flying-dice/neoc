import { LitElement, css, html, type TemplateResult } from 'lit'
import { styleMap } from 'lit/directives/style-map.js'
import './ui-icon'
import { define } from '../define'
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
  private hasAction = false
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
        align-items: center;
        gap: 10px;
        padding: 10px 12px;
        border-radius: var(--radius-md);
        /* Tinted in light mode; neutral in dark, where a tint under a plain action button costs its 4.5:1. */
        background: light-dark(
          color-mix(in srgb, var(--_color) 14%, var(--bg-grouped-elevated-secondary)),
          var(--bg-grouped-elevated-secondary)
        );
        box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--_color) 35%, transparent);
        font: var(--type-subheadline);
        letter-spacing: var(--tracking-subheadline);
        color: var(--label);
      }
      .icon {
        flex: none;
        display: flex;
        color: var(--_color);
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
      }
      /* no action: no gap for it either, so the padding is the same on both sides */
      .banner:not(.has-action) .action {
        display: none;
      }
      /* a button is 44 pt high (the hit area is layout space it owns): the banner's padding takes the extra */
      .action ::slotted(ui-button) {
        margin-block: -5px; /* drawn 34 pt high */
      }
      .action ::slotted(ui-button[size='small']) {
        margin-block: -8px;
      }
    `
  ]

  constructor() {
    super()
    this.tone = 'info'
    this.heading = ''
  }

  private onActionSlot(e: Event): void {
    this.hasAction = (e.target as HTMLSlotElement).assignedNodes().length > 0
    this.requestUpdate()
  }

  render(): TemplateResult {
    const tone: BannerTone = Object.hasOwn(ICONS, this.tone) ? this.tone : 'info'
    return html`<div
      class="banner ${this.hasAction ? 'has-action' : ''}"
      part="banner"
      role=${tone === 'error' ? 'alert' : 'status'}
      style=${styleMap({ '--_color': COLORS[tone] })}
    >
      <span class="icon" aria-hidden="true"><ui-icon name=${ICONS[tone]} size="20"></ui-icon></span>
      <span class="text">
        ${this.heading ? html`<span class="heading">${this.heading}</span>` : ''}
        <slot></slot>
      </span>
      <span class="action"
        ><slot name="action" @slotchange=${(e: Event) => this.onActionSlot(e)}></slot
      ></span>
    </div>`
  }
}

export type { BannerTone }

define('ui-banner', UiBanner)
declare global {
  interface HTMLElementTagNameMap {
    'ui-banner': UiBanner
  }
}
