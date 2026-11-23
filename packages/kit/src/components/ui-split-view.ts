import { LitElement, css, html, type TemplateResult } from 'lit'
import { define } from '../define'
import { base } from '../styles'

/** Under this width, in CSS pixels, a split view shows one column: the same for every app. */
const COLLAPSES_BELOW = 700

const COLUMNS = ['sidebar', 'content', 'detail'] as const

/**
 * A sidebar beside a detail (UISplitViewController, SwiftUI's `NavigationSplitView`), with a `content` column between
 * them when one is slotted. Given 700 pt or more it shows every column; given less it is `collapsed` and shows one:
 * the one `column` names (`NavigationSplitView`'s `preferredCompactColumn`). The app sets `column` as the person goes
 * in (they picked a row: `detail`) and back out.
 *
 * The way back is the app's, in the header of what it shows, as a navigation bar's back button is: draw it while the
 * split view is `collapsed`, and set `column` when it is pressed. Read `collapsed`, or hear `collapsedchange`, for that
 * and for whether a selection is worth showing in the sidebar (collapsed, the sidebar is a list to pick from).
 *
 * When the column in front changes while the keyboard is in the one that goes, the focus moves to the column that
 * comes, as iPadOS moves it into the view that was pushed: Tab goes on from there, and a screen reader keeps its place.
 * The column's element takes it, and is given `tabindex="-1"` for that, which it keeps (no ring is drawn around it).
 *
 * It fills the height its parent gives it, and each column gives its one element its whole box.
 *
 * @tag ui-split-view
 * @slot sidebar - The first column: what there is to pick from.
 * @slot content - A middle column, for three levels (folders, the notes of one, a note). Leave it out for two.
 * @slot - The detail: what was picked.
 * @fires {CustomEvent<{ collapsed: boolean }>} collapsedchange - The view went under or over the width at which it
 * shows one column. `collapsed` is already updated.
 * @cssprop --ui-split-sidebar-width - The width of the sidebar when there is room (default 280px).
 * @cssprop --ui-split-content-width - The width of the middle column when there is room (default 300px).
 */
export class UiSplitView extends LitElement {
  static properties = {
    /** The column in front while collapsed: `sidebar` (default), `content` or `detail`. */
    column: { type: String, reflect: true }
  }
  declare column: 'sidebar' | 'content' | 'detail'
  private narrow = false
  /** The width last told of with `collapsedchange`. */
  private told = false
  /**
   * Whether something is slotted as the middle column. A plain field that redraws by hand, not a reactive one: the
   * manifest would list a reactive one among the element's attributes, and it is nobody's to set.
   */
  private threeColumns = false
  /** Whether the focus was inside the view when this update began. */
  private hadFocus = false

  static styles = [
    base,
    css`
      :host {
        display: block;
        min-width: 0;
        min-height: 0;
        height: 100%;
      }
      .columns {
        display: grid;
        height: 100%;
        grid-template-columns:
          minmax(200px, var(--ui-split-sidebar-width, 280px))
          minmax(260px, 1fr);
      }
      .columns.three {
        grid-template-columns:
          minmax(200px, var(--ui-split-sidebar-width, 280px))
          minmax(220px, var(--ui-split-content-width, 300px))
          minmax(260px, 1fr);
      }
      slot {
        display: block;
        min-width: 0;
        min-height: 0;
      }
      slot[name] {
        border-inline-end: 1px solid var(--separator);
      }
      /* with nothing in it the middle column is not there */
      .columns:not(.three) slot[name='content'] {
        display: none;
      }
      ::slotted(*) {
        box-sizing: border-box;
        height: 100%;
      }
      /* one column: the one in front */
      :host([collapsed]) .columns {
        grid-template-columns: minmax(0, 1fr);
      }
      :host([collapsed]) slot {
        display: none;
        border-inline-end: 0;
      }
      :host([collapsed]) slot.front {
        display: block;
      }
      /* a column that was given the focus because it came to the front is not a control: no ring around the pane */
      ::slotted([tabindex='-1']:focus-visible) {
        outline: none !important;
      }
    `
  ]

  private observer: ResizeObserver | undefined

  constructor() {
    super()
    this.column = 'sidebar'
  }

  /**
   * Whether one column is shown. The element works it out from its own width, so it is read, never set; the host also
   * carries a `collapsed` attribute while it is true, for a style to select on.
   */
  get collapsed(): boolean {
    return this.narrow
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0
      // not laid out (hidden, or not drawn yet) is not narrow
      if (width > 0) this.measured(width < COLLAPSES_BELOW)
    })
    this.observer.observe(this)
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.observer?.disconnect()
    this.observer = undefined
  }

  protected willUpdate(): void {
    this.hadFocus = this.matches(':focus-within')
  }

  protected updated(): void {
    this.keepFocus()
    // heard once the view is redrawn for the width it tells of
    if (this.told === this.narrow) return
    this.told = this.narrow
    this.dispatchEvent(
      new CustomEvent('collapsedchange', {
        detail: { collapsed: this.narrow },
        bubbles: true,
        composed: true
      })
    )
  }

  /** The width was measured: when it crosses the breakpoint the view redraws. */
  private measured(narrow: boolean): void {
    if (narrow === this.narrow) return
    this.narrow = narrow
    this.toggleAttribute('collapsed', narrow)
    this.requestUpdate()
  }

  /** The focus was in a column that is no longer shown: the column in front takes it. */
  private keepFocus(): void {
    if (!this.collapsed || !this.hadFocus) return
    const column = this.renderRoot
      .querySelector<HTMLSlotElement>('slot.front')
      ?.assignedElements()[0]
    if (!(column instanceof HTMLElement) || column.matches(':focus-within')) return
    if (!column.hasAttribute('tabindex')) column.tabIndex = -1
    column.focus({ preventScroll: true })
  }

  /** The column in front: the one asked for, or the nearest there is (no middle column: the detail). */
  private get front(): (typeof COLUMNS)[number] {
    const asked = COLUMNS.includes(this.column) ? this.column : 'sidebar'
    return asked === 'content' && !this.threeColumns ? 'detail' : asked
  }

  private onContentSlot = (e: Event): void => {
    const three = (e.target as HTMLSlotElement).assignedElements().length > 0
    if (three === this.threeColumns) return
    this.threeColumns = three
    this.requestUpdate()
  }

  render(): TemplateResult {
    const front = this.front
    const at = (column: (typeof COLUMNS)[number]): string => (column === front ? 'front' : '')
    return html`<div class="columns ${this.threeColumns ? 'three' : ''}">
      <slot name="sidebar" class=${at('sidebar')}></slot>
      <slot name="content" class=${at('content')} @slotchange=${this.onContentSlot}></slot>
      <slot class=${at('detail')}></slot>
    </div>`
  }
}

export type SplitViewColumn = UiSplitView['column']

define('ui-split-view', UiSplitView)
declare global {
  interface HTMLElementTagNameMap {
    'ui-split-view': UiSplitView
  }
}
