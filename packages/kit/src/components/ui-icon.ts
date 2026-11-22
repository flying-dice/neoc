import { LitElement, css, svg } from 'lit'
import { unsafeSVG } from 'lit/directives/unsafe-svg.js'
import { icons } from 'kit:icons'
import { base } from '../styles'

/**
 * A glyph from the vos icon set (the same set the shell draws).
 *
 * Decorative unless it has a `label`, then it is an image with that name. The markup comes only from the constant
 * icon map, looked up by `name`; an unknown name draws nothing, and no app text is ever read as markup.
 *
 * @tag ui-icon
 */
export class UiIcon extends LitElement {
  static properties = {
    /** The glyph's name, for example `chevron.right` or `sparkles`. */
    name: { type: String, reflect: true },
    /** Width and height in px. */
    size: { type: Number },
    /** Stroke weight, as SF Symbols weights: 1.5 light, 2 regular, 2.5 semibold. */
    weight: { type: Number },
    /** An accessible name; leave it out for a decorative icon. */
    label: { type: String }
  }
  declare name: string
  declare size: number
  declare weight: number
  declare label: string

  static styles = [
    base,
    css`
      :host {
        display: inline-flex;
        flex: none;
        vertical-align: middle;
        color: inherit;
      }
      svg {
        display: block;
      }
    `
  ]

  constructor() {
    super()
    this.name = ''
    this.size = 22
    this.weight = 2
    this.label = ''
  }

  render() {
    const glyph = Object.hasOwn(icons, this.name) ? icons[this.name] : undefined
    return svg`<svg
      width=${this.size}
      height=${this.size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      stroke-width=${this.weight}
      stroke-linecap="round"
      stroke-linejoin="round"
      role=${this.label ? 'img' : 'presentation'}
      aria-label=${this.label || ''}
      aria-hidden=${this.label ? 'false' : 'true'}
    >${glyph ? unsafeSVG(glyph) : ''}</svg>`
  }
}

customElements.define('ui-icon', UiIcon)
declare global {
  interface HTMLElementTagNameMap {
    'ui-icon': UiIcon
  }
}
