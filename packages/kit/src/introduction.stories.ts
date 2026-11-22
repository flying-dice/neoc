import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import 'virtual:kit-tokens.css'

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
