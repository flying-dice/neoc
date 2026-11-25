import { LitElement, css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { base } from '../styles'

/**
 * How full something is: a track with a fill from the start (a usage gauge, as in Settings › Storage). It is a
 * horizontal bar, or with `orientation="vertical"` a column that fills from the bottom, for a row of them side by side.
 *
 * Without a `value` the track is empty, for a reading that has not been taken yet. With one it is a `meter`: it
 * says its value as a percentage to a screen reader, under `label`.
 *
 * @tag ui-meter
 * @csspart track - The empty track.
 * @csspart fill - The filled part.
 */
export class UiMeter extends LitElement {
  static properties = {
    /** How full, from 0 to 100. Left out, the track is empty and the meter is not announced. */
    value: { type: Number },
    /** The accessible name: what is measured (`Memory`). */
    label: { type: String },
    /** `horizontal` (default) or `vertical`, which fills from the bottom and takes the height it is given. */
    orientation: { type: String, reflect: true }
  }
  declare value: number | undefined
  declare label: string
  declare orientation: 'horizontal' | 'vertical'

  static styles = [
    base,
    css`
      :host {
        display: block;
        height: 6px;
        min-width: 0;
      }
      :host([orientation='vertical']) {
        height: 64px;
        width: 100%;
      }
      .track {
        position: relative;
        width: 100%;
        height: 100%;
        overflow: hidden;
        border-radius: var(--radius-pill);
        background: var(--fill-tertiary);
      }
      :host([orientation='vertical']) .track {
        border-radius: var(--radius-xs);
      }
      .fill {
        position: absolute;
        left: 0;
        top: 0;
        bottom: 0;
        border-radius: inherit;
        background: var(--tint);
      }
      :host([orientation='vertical']) .fill {
        top: auto;
        right: 0;
        width: auto;
      }
      @media (prefers-reduced-motion: no-preference) {
        .fill {
          transition:
            width 0.4s ease,
            height 0.4s ease;
        }
      }
    `
  ]

  constructor() {
    super()
    this.value = undefined
    this.label = ''
    this.orientation = 'horizontal'
  }

  /** The value as the fill shows it: whole, and kept between empty and full. */
  private get percent(): number | undefined {
    const value = this.value
    if (value === undefined || value === null || Number.isNaN(value)) return undefined
    return Math.min(100, Math.max(0, value))
  }

  render(): TemplateResult {
    const percent = this.percent
    const fill =
      percent === undefined
        ? ''
        : this.orientation === 'vertical'
          ? `height:${percent}%`
          : `width:${percent}%`
    return html`<div
      class="track"
      part="track"
      role=${percent === undefined ? 'presentation' : 'meter'}
      aria-label=${ifDefined(percent === undefined || !this.label ? undefined : this.label)}
      aria-valuemin=${ifDefined(percent === undefined ? undefined : '0')}
      aria-valuemax=${ifDefined(percent === undefined ? undefined : '100')}
      aria-valuenow=${ifDefined(percent === undefined ? undefined : String(Math.round(percent)))}
      aria-valuetext=${ifDefined(percent === undefined ? undefined : `${Math.round(percent)} percent`)}
    >
      ${percent === undefined ? '' : html`<div class="fill" part="fill" style=${fill}></div>`}
    </div>`
  }
}

define('ui-meter', UiMeter)
declare global {
  interface HTMLElementTagNameMap {
    'ui-meter': UiMeter
  }
}
