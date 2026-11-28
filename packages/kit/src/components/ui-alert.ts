import { LitElement, css, html, nothing, type TemplateResult } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { define } from '../define'
import { base } from '../styles'
import './ui-button'
import type { UiButton } from './ui-button'

/**
 * An alert that asks before something is done (UIAlertController's alert): a heading, a message, Cancel and the answer
 * that goes ahead. While `open` it is modal: the window behind it is dimmed and inert, the focus stays inside and goes
 * back where it was when the alert closes.
 *
 * The answer that changes nothing is in front. With `destructive` the answer that goes ahead is red, and Cancel is
 * bold and has the focus, so a stray Return deletes nothing; without it the answer that goes ahead is bold and has the
 * focus.
 * Escape, and the browser's own request to close (a back gesture), answer Cancel. So does hiding what holds the alert
 * while it is open: a question nobody can see is not left over an inert page.
 *
 * An answer closes the alert (`open` becomes false, the focus goes back) and then raises its event.
 *
 * The two answers have the test ids `<data-testid of the alert>-cancel` and `<data-testid of the alert>-confirm`.
 *
 * @tag ui-alert
 * @fires {Event} confirm - The person chose the answer that goes ahead. The alert has closed. It does not bubble.
 * @fires {Event} cancel - Cancel, Escape or the browser's request to close. The alert has closed. It does not bubble.
 * @csspart cancel - The Cancel button.
 * @csspart action - The button that goes ahead.
 */
export class UiAlert extends LitElement {
  static properties = {
    /** Shows the alert, over everything else in the window. */
    open: { type: Boolean, reflect: true },
    /** The question: `Delete Work?` */
    heading: { type: String },
    /** What going ahead does, under the heading. */
    message: { type: String },
    /** The word on the button that goes ahead (default `OK`). */
    actionLabel: { type: String, attribute: 'action-label' },
    /** The word on the button that changes nothing (default `Cancel`). */
    cancelLabel: { type: String, attribute: 'cancel-label' },
    /** Going ahead deletes something or cannot be undone: its button is red, and Cancel has the focus. */
    destructive: { type: Boolean, reflect: true },
    /** The test id; the answers' are this followed by `-cancel` and `-confirm`. */
    testid: { type: String, attribute: 'data-testid' }
  }
  declare open: boolean
  declare heading: string
  declare message: string
  declare actionLabel: string
  declare cancelLabel: string
  declare destructive: boolean
  declare testid: string | null

  static styles = [
    base,
    css`
      /* the alert is in the top layer: the element itself takes no room where it is written */
      :host {
        display: contents;
      }
      dialog {
        width: min(270px, calc(100vw - 32px));
        max-height: calc(100vh - 32px);
        padding: 0;
        border: 0;
        border-radius: var(--radius-xl);
        background: var(--material-thick);
        backdrop-filter: var(--material-blur);
        color: var(--label);
        box-shadow: var(--shadow-popover);
        overflow: hidden;
      }
      dialog[open] {
        display: flex;
        flex-direction: column;
      }
      dialog::backdrop {
        background: var(--dimming);
      }
      .text {
        min-height: 0;
        overflow-y: auto;
        padding: 20px 16px 16px;
        text-align: center;
        overflow-wrap: anywhere;
      }
      h2 {
        margin: 0;
        font: var(--type-headline);
        letter-spacing: var(--tracking-body);
      }
      p {
        margin: 4px 0 0;
        font: var(--type-footnote);
        letter-spacing: var(--tracking-footnote);
      }
      /* the two answers share the width, with a hairline over them and between them */
      .answers {
        flex: none;
        display: grid;
        grid-template-columns: 1fr 1fr;
        border-top: 1px solid var(--separator);
      }
      /* each answer fills its half, square: the alert's own corners round them, so the ring is drawn inside */
      ui-button::part(button) {
        border-radius: 0;
        font-weight: 400;
        outline-offset: -3px;
      }
      ui-button + ui-button {
        border-inline-start: 1px solid var(--separator);
      }
      ui-button.preferred::part(button) {
        font-weight: 600;
      }
    `
  ]

  constructor() {
    super()
    this.open = false
    this.heading = ''
    this.message = ''
    this.actionLabel = 'OK'
    this.cancelLabel = 'Cancel'
    this.destructive = false
    this.testid = null
  }

  /** What had the focus when the alert opened, for the one case the dialog cannot give it back itself. */
  private opener: Element | null = null

  /** Watches that the open alert is still drawn. */
  private drawn: ResizeObserver | undefined

  private get dialog(): HTMLDialogElement | null {
    return this.renderRoot.querySelector('dialog')
  }

  connectedCallback(): void {
    super.connectedCallback()
    // moved while open: a dialog shows as a modal only once it is in a document again
    if (this.hasUpdated) this.show()
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    if (!this.dialog?.open) return
    // taken out of the page while open: a dialog that is gone gives no focus back, so it is given back here
    this.dialog.close()
    this.drawn?.disconnect()
    if (this.opener instanceof HTMLElement && this.opener.isConnected) this.opener.focus()
  }

  /** The element with the focus, through every shadow root on the way to it. */
  private static focused(): Element | null {
    let at = document.activeElement
    while (at?.shadowRoot?.activeElement) at = at.shadowRoot.activeElement
    return at
  }

  /**
   * An open alert that stops being drawn (what holds it was hidden: a column of a split view that went out of front)
   * would leave the page inert behind a question nobody can see, so it is answered Cancel.
   */
  private watchDrawn(dialog: HTMLDialogElement): void {
    this.drawn?.disconnect()
    this.drawn = new ResizeObserver(() => {
      if (this.open && dialog.open && dialog.getClientRects().length === 0) this.answer('cancel')
    })
    this.drawn.observe(dialog)
  }

  protected updated(): void {
    this.show()
  }

  /** Brings the dialog in step with `open`, and puts the focus on the answer in front when it opens. */
  private show(): void {
    const dialog = this.dialog
    if (!dialog || !this.isConnected || dialog.open === this.open) return
    if (!this.open) {
      this.drawn?.disconnect()
      dialog.close()
      return
    }
    this.opener = UiAlert.focused()
    dialog.showModal()
    this.watchDrawn(dialog)
    const front = this.renderRoot.querySelector<UiButton>('ui-button.preferred')
    if (!front) return
    // at once, so that a key pressed next goes where the focus is seen to be; an alert drawn open waits for its
    // buttons, which draw their own button a moment after they are made
    if (front.hasUpdated) front.focus()
    else
      void front.updateComplete.then(() => {
        if (this.open && dialog.open) front.focus()
      })
  }

  private answer(type: 'confirm' | 'cancel'): void {
    if (!this.open) return
    this.open = false
    // closed now, not at the next update: the focus is back where it was before anyone hears the answer
    this.drawn?.disconnect()
    if (this.dialog?.open) this.dialog.close()
    // heard on the alert only, as a dialog's own `cancel` is: an answer is not for an ancestor (a dialog around it
    // would take a bubbling `cancel` for its own)
    this.dispatchEvent(type === 'confirm' ? new Event('confirm') : new Event('cancel'))
  }

  private onKeydown = (e: KeyboardEvent): void => {
    if (e.key !== 'Escape') return
    e.preventDefault()
    this.answer('cancel')
  }

  /** The browser asks to close a modal dialog (Escape, a back gesture): that is Cancel, and the alert closes itself. */
  private onDialogCancel = (e: Event): void => {
    e.preventDefault()
    this.answer('cancel')
  }

  /**
   * The browser closed it without asking (it may, the second time it is asked in a row): still Cancel. `close` is told
   * late, so one from an answer already given can arrive after the alert was opened again: the dialog is open then, and
   * there is nothing to answer. Taken out of the page while open is not an answer either: it opens again when put back.
   */
  private onDialogClose = (): void => {
    if (this.isConnected && this.dialog?.open === false) this.answer('cancel')
  }

  render(): TemplateResult {
    return html`<dialog
      role="alertdialog"
      aria-labelledby="heading"
      aria-describedby=${this.message ? 'message' : nothing}
      @keydown=${this.onKeydown}
      @cancel=${this.onDialogCancel}
      @close=${this.onDialogClose}
    >
      <div class="text">
        <h2 id="heading">${this.heading}</h2>
        ${this.message ? html`<p id="message">${this.message}</p>` : nothing}
      </div>
      <div class="answers">
        <ui-button
          part="cancel"
          data-testid=${ifDefined(this.testid ? `${this.testid}-cancel` : undefined)}
          class=${this.destructive ? 'preferred' : ''}
          variant="plain"
          size="fill"
          @click=${() => this.answer('cancel')}
          >${this.cancelLabel}</ui-button
        >
        <ui-button
          part="action"
          data-testid=${ifDefined(this.testid ? `${this.testid}-confirm` : undefined)}
          class=${this.destructive ? '' : 'preferred'}
          variant="plain"
          size="fill"
          ?destructive=${this.destructive}
          @click=${() => this.answer('confirm')}
          >${this.actionLabel}</ui-button
        >
      </div>
    </dialog>`
  }
}

define('ui-alert', UiAlert)
declare global {
  interface HTMLElementTagNameMap {
    'ui-alert': UiAlert
  }
}
