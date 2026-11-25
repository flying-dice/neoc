import { LitElement, css, html, type TemplateResult } from 'lit'
import { define } from '../define'
import { base } from '../styles'

/**
 * Two values at the end of a `ui-row`, right-aligned in fixed columns so that the rows of a list line up: a main
 * value in headline type and a secondary one in muted type (a process's CPU and memory, a name and its size). With
 * `layout="stacked"` the secondary value sits under the main one, for a narrow window.
 *
 * @tag ui-value-pair
 * @csspart main - The main value.
 * @csspart secondary - The secondary value.
 */
export class UiValuePair extends LitElement {
  static properties = {
    /** The main value. A dash (`—`) says it is not there yet, and is muted. */
    main: { type: String },
    /** The secondary value. */
    secondary: { type: String },
    /** `inline` (default): side by side in columns. `stacked`: one under the other. */
    layout: { type: String, reflect: true },
    /** The accessible name for the two together when it should differ from reading them in turn. */
    label: { type: String }
  }
  declare main: string
  declare secondary: string
  declare layout: 'inline' | 'stacked'
  declare label: string

  static styles = [
    base,
    css`
      :host {
        display: flex;
        gap: 16px;
        text-align: right;
        font-variant-numeric: tabular-nums;
      }
      :host([layout='stacked']) {
        flex-direction: column;
        gap: 0;
      }
      .main {
        min-width: 56px;
        font: var(--type-headline);
        letter-spacing: var(--tracking-body);
        color: var(--label);
      }
      .main.none {
        color: var(--label-secondary);
      }
      .secondary {
        min-width: 64px;
        font: var(--type-subheadline);
        letter-spacing: var(--tracking-subheadline);
        color: var(--label-secondary);
      }
      :host([layout='stacked']) .main,
      :host([layout='stacked']) .secondary {
        min-width: 0;
      }
      :host([layout='stacked']) .secondary {
        font: var(--type-footnote);
        letter-spacing: var(--tracking-footnote);
      }
    `
  ]

  constructor() {
    super()
    this.main = ''
    this.secondary = ''
    this.layout = 'inline'
    this.label = ''
  }

  render(): TemplateResult {
    const none = this.main === '—'
    return html`<span
        class="main ${none ? 'none' : ''}"
        part="main"
        aria-label=${this.label || null}
        >${this.main}</span
      ><span class="secondary" part="secondary">${this.secondary}</span>`
  }
}

define('ui-value-pair', UiValuePair)
declare global {
  interface HTMLElementTagNameMap {
    'ui-value-pair': UiValuePair
  }
}
