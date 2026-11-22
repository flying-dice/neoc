import { css, html, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { colourVars } from '../style'
import { BooleanControl } from '../boolean-control'
import { base } from '../styles'

/**
 * An on/off switch (UISwitch). A form control: with a `name` it submits `value` while on.
 *
 * @tag ui-toggle
 * @fires {CustomEvent<{ checked: boolean }>} change - The person flipped it. `checked` is already updated.
 * @csspart switch - The inner switch button.
 * @cssprop --toggle-tint - The on colour; defaults to system green.
 */
export class UiToggle extends BooleanControl {
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
        border: 0;
        padding: 0;
        position: relative; /* the hit area below */
        flex: none;
        width: 51px;
        height: 31px;
        border-radius: 999px;
        background: var(--switch-off);
        cursor: pointer;
        transition: background-color var(--duration) var(--ease-ios);
      }
      button[aria-checked='true'] {
        background: var(--toggle-tint, var(--ios-green));
      }
      button:disabled {
        opacity: 0.4;
        cursor: default;
      }
      /* a 44 x 44 pt hit area around the drawn switch, which keeps its size */
      button::after {
        content: '';
        position: absolute;
        left: 50%;
        top: 50%;
        width: max(100%, 44px);
        height: max(100%, 44px);
        transform: translate(-50%, -50%);
      }
      .knob {
        position: absolute;
        top: 2px;
        left: 2px;
        width: 27px;
        height: 27px;
        border-radius: 999px;
        background: var(--knob);
        box-shadow:
          var(--shadow-control),
          0 0 0 0.5px rgb(0 0 0 / 0.04);
        transition:
          transform var(--duration) var(--ease-ios),
          width var(--duration-fast) var(--ease-ios);
      }
      [aria-checked='true'] .knob {
        transform: translateX(20px);
      }
      /* Press stretches the knob, like iOS. */
      button:active:not(:disabled) .knob {
        width: 33px;
      }
      [aria-checked='true']:active:not(:disabled) .knob {
        transform: translateX(14px);
      }
    `
  ]

  render(): TemplateResult {
    return html`<button
      part="switch"
      type="button"
      role="switch"
      aria-checked=${this.checked ? 'true' : 'false'}
      aria-label=${ifDefined(this.accessibleName)}
      aria-description=${ifDefined(this.description || undefined)}
      ?disabled=${this.isDisabled}
      style=${colourVars({ '--toggle-tint': this.tint })}
      @click=${() => this.flip()}
    >
      <span class="knob"></span>
    </button>`
  }
}

define('ui-toggle', UiToggle)
declare global {
  interface HTMLElementTagNameMap {
    'ui-toggle': UiToggle
  }
}
