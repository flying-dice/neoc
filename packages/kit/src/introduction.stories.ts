import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import 'virtual:kit-tokens.css'
import './components/ui-banner'
import './components/ui-button'

const meta: Meta = { title: 'Introduction/Tokens', parameters: { controls: { disable: true }, layout: 'padded' } }
export default meta

const swatches = ['blue', 'green', 'indigo', 'orange', 'pink', 'purple', 'red', 'teal', 'yellow', 'mint', 'cyan', 'brown']

export const Colours: StoryObj = {
  render: () => html`<div style="display:grid;grid-template-columns:repeat(6,72px);gap:12px;font:var(--type-caption1);color:var(--label)">
    ${swatches.map(
      (c) => html`<div data-testid="swatch-${c}"><div style="height:44px;border-radius:var(--radius-sm);background:var(--ios-${c})"></div>${c}</div>`
    )}
  </div>`
}

export const Type: StoryObj = {
  render: () => html`<div style="color:var(--label);display:grid;gap:6px">
    <div style="font:var(--type-large-title)">Large title</div>
    <div style="font:var(--type-title2)">Title 2</div>
    <div style="font:var(--type-headline)">Headline</div>
    <div style="font:var(--type-body)">Body</div>
    <div style="font:var(--type-subheadline);color:var(--label-secondary)">Subheadline</div>
    <div style="font:var(--type-footnote);color:var(--label-secondary)">Footnote</div>
  </div>`
}

/**
 * An app opened on its own (`/apps/<id>` full screen) has no shell to set `data-theme`: the same tokens then follow the
 * person's system setting. This box sets no theme of its own (`color-scheme: light dark`), so it shows what such a page
 * looks like under the system setting the browser runs with, whatever the theme switch above says. An explicit
 * `data-theme` always wins, which the frame-conditions spec (packages/kit/e2e) checks on whole documents.
 */
export const FollowsTheSystemWithNoDataTheme: StoryObj = {
  render: () => html`<div
    data-testid="system-theme"
    style="color-scheme: light dark;background:var(--bg-grouped);color:var(--label);padding:16px;border-radius:var(--radius-lg);display:grid;gap:12px;width:320px"
  >
    <span style="font:var(--type-footnote);color:var(--label-secondary)">No data-theme: the system setting decides</span>
    <ui-button data-testid="system-button">Continue</ui-button>
    <ui-banner data-testid="system-banner">Standalone and embedded look alike.</ui-banner>
  </div>`
}
