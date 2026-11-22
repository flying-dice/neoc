import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import './ui-segmented'

const filters = [
  { value: 'all', label: 'All' },
  { value: 'active', label: 'Active' },
  { value: 'done', label: 'Done' }
]

const meta: Meta = {
  title: 'Controls/Segmented',
  component: 'ui-segmented',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

export const Basic: Story = {
  render: () => html`<ui-segmented data-testid="seg" label="Filter" value="all" .options=${filters}></ui-segmented>`
}
export const Block: Story = {
  render: () => html`<div style="width:320px"><ui-segmented data-testid="seg-block" label="Filter" block value="active" .options=${filters}></ui-segmented></div>`
}
export const WithDisabledSegment: Story = {
  render: () => html`<ui-segmented
    data-testid="seg-disabled"
    label="Filter"
    value="all"
    .options=${[...filters.slice(0, 2), { ...filters[2]!, disabled: true }]}
  ></ui-segmented>`
}
export const Disabled: Story = {
  render: () => html`<ui-segmented data-testid="seg-off" label="Filter" disabled value="all" .options=${filters}></ui-segmented>`
}
export const NothingSelected: Story = {
  render: () => html`<ui-segmented data-testid="seg-none" label="Filter" .options=${filters}></ui-segmented>`
}

export const Choosing: Story = {
  render: () => html`<ui-segmented data-testid="choose" label="Filter" value="all" .options=${filters}></ui-segmented>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const seg = canvas.getByTestId('choose') as HTMLElement & { value: string }
    const onChange = fn()
    seg.addEventListener('change', onChange)
    await userEvent.click(canvas.getByTestId('choose-active'))
    await expect(seg.value).toBe('active')
    await expect(onChange).toHaveBeenLastCalledWith(expect.objectContaining({ detail: { value: 'active' } }))
    // the arrow keys move the selection and focus together
    await userEvent.keyboard('{ArrowRight}')
    await expect(seg.value).toBe('done')
    await userEvent.keyboard('{ArrowRight}')
    await expect(seg.value).toBe('all')
    await userEvent.keyboard('{End}')
    await expect(seg.value).toBe('done')
    await expect(canvas.getByTestId('choose-done')).toHaveAttribute('aria-checked', 'true')
  }
}
