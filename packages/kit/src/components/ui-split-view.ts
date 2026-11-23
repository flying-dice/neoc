import { LitElement, css, html, type TemplateResult } from 'lit'
import { define } from '../define'
import { base } from '../styles'

/** Under this width, in CSS pixels, a split view shows one column: the same for every app. */
export const SPLIT_VIEW_COLLAPSES_BELOW = 700

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
 * It fills the height its parent gives it, and each column gives its one element its whole box.
 *
 * @tag ui-split-view
 * @slot sidebar - The first column: what there is to pick from.
 * @slot content - A middle column, for three levels (folders, the notes of one, a note). Leave it out for two.
 * @slot - The detail: what was picked.
 * @fires {CustomEvent<{ collapsed: boolean }>} collapsedchange - The view went under or over the width at which it shows one column. `collapsed` is already updated.
 * @cssprop --ui-split-sidebar-width - The width of the sidebar when there is room (default 280px).
 * @cssprop --ui-split-content-width - The width of the middle column when there is room (default 300px).
 */
export class UiSplitView extends LitElement {
  static properties = {
    /** The column in front while collapsed: `sidebar` (default), `content` or `detail`. */
    column: { type: String, reflect: true },
    /** Whether one column is shown. The element sets it from its own width: read it, do not set it. */
    collapsed: { type: Boolean, reflect: true }
  }
  declare column: 'sidebar' | 'content' | 'detail'
  declare collapsed: boolean
  /** Whether something is slotted as the middle column. Not a property of the element: it redraws by hand. */
  private threeColumns = false

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
    `
  ]

  private observer: ResizeObserver | undefined

  constructor() {
    super()
    this.column = 'sidebar'
    this.collapsed = false
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.observer = new ResizeObserver(([entry]) => {
      const width = entry?.contentRect.width ?? 0
      // not laid out (hidden, or not drawn yet) is not narrow
      if (width > 0) this.collapsed = width < SPLIT_VIEW_COLLAPSES_BELOW
    })
    this.observer.observe(this)
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.observer?.disconnect()
    this.observer = undefined
  }

  protected updated(changed: Map<PropertyKey, unknown>): void {
    // the first value (not collapsed) is not a change; the first measurement that differs is
    if (changed.has('collapsed') && changed.get('collapsed') !== undefined)
      this.dispatchEvent(
        new CustomEvent('collapsedchange', {
          detail: { collapsed: this.collapsed },
          bubbles: true,
          composed: true
        })
      )
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
