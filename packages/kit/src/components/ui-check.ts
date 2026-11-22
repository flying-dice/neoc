import { css, html } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { BooleanControl } from '../boolean-control'
import { base } from '../styles'

/**
 * A round check (the selection mark of a list row, a task done). A form control: with a `name` it submits `value`
 * while checked.
 *
 * @tag ui-check
 * @fires {CustomEvent<{ checked: boolean }>} change - The person flipped it. `checked` is already updated.
 * @csspart box - The inner check button.
 * @cssprop --toggle-tint - The checked colour; defaults to the accent.
 */
export class UiCheck extends BooleanControl {
  static styles = [
    base,
    css`
      :host {
        display: inline-flex;
        vertical-align: middle;
        flex: none;
      }
      :host([disabled]) {
        pointer-events: none;
      }
      button {
        appearance: none;
        flex: none;
        display: grid;
        place-items: center;
        width: 24px;
        height: 24px;
        padding: 0;
        border-radius: 50%;
        border: 2px solid var(--label-secondary);
        background: transparent;
        color: transparent;
        cursor: pointer;
        transition:
          background-color var(--duration-fast) var(--ease-ios),
          border-color var(--duration-fast) var(--ease-ios);
      }
      button[aria-checked='true'] {
        border-color: transparent;
        background: var(--toggle-tint, var(--tint-fill));
        color: #fff;
      }
      button:disabled {
        opacity: 0.4;
        cursor: default;
      }
      svg {
        display: block;
      }
    `
  ]

  render() {
    return html`<button
      part="box"
      type="button"
      role="checkbox"
      aria-checked=${this.checked ? 'true' : 'false'}
      aria-label=${ifDefined(this.accessibleName)}
      aria-description=${ifDefined(this.description || undefined)}
      ?disabled=${this.isDisabled}
      style=${ifDefined(this.tint ? `--toggle-tint:${this.tint}` : undefined)}
      @click=${() => this.flip()}
    >
      <svg viewBox="0 0 24 24" width="16" height="16" fill="none" stroke="currentColor" stroke-width="3.2" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true">
        <path d="M5 12.5l4.5 4.5L19 7" />
      </svg>
    </button>`
  }
}

customElements.define('ui-check', UiCheck)
declare global {
  interface HTMLElementTagNameMap {
    'ui-check': UiCheck
  }
}
