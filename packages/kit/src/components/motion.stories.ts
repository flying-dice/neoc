import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, within } from 'storybook/test'
import './ui-button'
import './ui-segmented'
import './ui-spinner'
import './ui-toggle'

const meta: Meta = {
  title: 'Feedback/Reduce Motion',
  parameters: { controls: { disable: true } }
}
export default meta

/**
 * Under Reduce Motion no transition starts at all, and a busy indicator keeps turning. The end-to-end run emulates it for
 * every story, so this one holds the components to it by reading what the browser computed inside each shadow root.
 */
export const NothingMovesButTheSpinner: StoryObj = {
  render: () => html`<div style="display:flex;gap:20px;align-items:center">
    <ui-button data-testid="motion-button">Button</ui-button>
    <ui-toggle data-testid="motion-toggle" label="Toggle" checked></ui-toggle>
    <ui-segmented data-testid="motion-segmented" label="Choice" value="a" .options=${[{ value: 'a', label: 'A' }, { value: 'b', label: 'B' }]}></ui-segmented>
    <ui-spinner data-testid="motion-spinner"></ui-spinner>
  </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const inside = (id: string, selector: string): CSSStyleDeclaration =>
      getComputedStyle(canvas.getByTestId(id).shadowRoot!.querySelector(selector)!)
    const reduced = matchMedia('(prefers-reduced-motion: reduce)').matches
    await expect(inside('motion-button', 'button').transitionDuration).toBe(reduced ? '0s' : '0.18s, 0.18s')
    await expect(inside('motion-toggle', '.knob').transitionDuration).toBe(reduced ? '0s' : '0.3s, 0.18s')
    await expect(inside('motion-segmented', '.thumb').transitionDuration).toBe(reduced ? '0s' : '0.3s')
    await expect(inside('motion-spinner', 'svg').animationDuration).toBe('0.8s') // busy: it keeps turning either way
  }
}
