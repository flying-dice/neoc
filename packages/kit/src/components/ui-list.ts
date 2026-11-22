import { LitElement, css, html } from 'lit'
import { ifDefined } from 'lit/directives/if-defined.js'
import { moveIndex } from 'kit:keys'
import type { UiRow } from './ui-row'
import type { ListVariant, UiSection } from './ui-section'
import './ui-row'
import './ui-section'
import { base } from '../styles'

export type { ListVariant } from './ui-section'

/**
 * A list of rows in `ui-section`s, in one of the five iPadOS styles. It owns what the rows share: the list
 * semantics (a `listbox` when rows are selectable), one tab stop with arrow-key navigation (Up, Down, Home, End), and
 * the look of its sections.
 *
 * Selection is controlled: set `selected` on the rows you want marked and listen for `click` on a row.
 *
 * @tag ui-list
 * @slot - `ui-section`s, or `ui-row`s directly for a list with no headers.
 */
export class UiList extends LitElement {
  static properties = {
    /** `insetGrouped` (default), `inset`, `plain`, `grouped` or `sidebar`. */
    variant: { type: String, reflect: true },
    /** `none` (default), `single` or `multi`: the rows are options of a `listbox`. */
    selection: { type: String, reflect: true },
    /** The accessible name of the list. */
    label: { type: String }
  }
  declare variant: 'insetGrouped' | 'inset' | 'plain' | 'grouped' | 'sidebar'
  declare selection: 'none' | 'single' | 'multi'
  declare label: string
  private direct = false
  /** Rows and sections come and go (an app renders them from its data): the list follows. */
  private observer = new MutationObserver(() => this.refresh())

  static styles = [
    base,
    css`
      :host {
        display: block;
        min-width: 0;
      }
    `
  ]

  constructor() {
    super()
    this.variant = 'insetGrouped'
    this.selection = 'none'
    this.label = ''
  }

  private get selectable(): boolean {
    return this.selection === 'single' || this.selection === 'multi'
  }

  private get rows(): UiRow[] {
    return Array.from(this.querySelectorAll<UiRow>('ui-row'))
  }

  private get stops(): UiRow[] {
    return this.rows.filter((r) => r.focusable)
  }

  connectedCallback(): void {
    super.connectedCallback()
    this.addEventListener('focusin', this.onFocusIn)
    this.addEventListener('keydown', this.onKeydown)
    this.observer.observe(this, { childList: true, subtree: true })
  }

  disconnectedCallback(): void {
    super.disconnectedCallback()
    this.observer.disconnect()
    this.removeEventListener('focusin', this.onFocusIn)
    this.removeEventListener('keydown', this.onKeydown)
  }

  /** Exactly one row is in the tab order: the one with focus, else the selected one, else the first. */
  private settleTabStop(chosen?: UiRow): void {
    const stops = this.stops
    const stop = chosen ?? stops.find((r) => r.tabStop) ?? stops.find((r) => r.selected) ?? stops[0]
    for (const row of this.rows) {
      const next = row === stop
      if (row.tabStop !== next || chosen) {
        row.tabStop = next
        row.requestUpdate()
      }
    }
  }

  private onFocusIn = (e: Event): void => {
    const row = (e.target as HTMLElement).closest<UiRow>('ui-row')
    if (row?.focusable) this.settleTabStop(row)
  }

  private onKeydown = (e: KeyboardEvent): void => {
    // a field inside a row (a ui-text-field's input is in its shadow root) keeps its own arrow keys
    const path = e.composedPath().filter((n): n is HTMLElement => n instanceof HTMLElement)
    if (path.some((n) => n.matches('input, textarea, select, [contenteditable]'))) return
    const stops = this.stops
    const here = stops.findIndex((r) => r === (e.target as HTMLElement).closest('ui-row'))
    if (here < 0) return
    const to = moveIndex(stops.length, here, e.key, 'vertical', false)
    if (to === null) return
    e.preventDefault()
    stops[to]?.focusRow()
  }

  private refresh(): void {
    const sections = Array.from(this.querySelectorAll<UiSection>('ui-section'))
    for (const section of sections) {
      section.variant = this.variant
      section.requestUpdate()
    }
    const direct = this.querySelector(':scope > ui-row') !== null
    if (direct !== this.direct) {
      this.direct = direct
      this.requestUpdate()
    }
    this.settleTabStop()
    for (const row of this.rows) row.requestUpdate()
  }

  protected updated(): void {
    this.refresh()
  }

  render() {
    const role = this.selectable ? 'listbox' : this.direct ? 'list' : 'group'
    return html`<div
      role=${role}
      aria-label=${ifDefined(this.label || undefined)}
      aria-multiselectable=${ifDefined(this.selection === 'multi' ? 'true' : undefined)}
    >
      <slot @slotchange=${() => this.refresh()}></slot>
    </div>`
  }
}

export type ListSelection = UiList['selection']

customElements.define('ui-list', UiList)
declare global {
  interface HTMLElementTagNameMap {
    'ui-list': UiList
  }
}
