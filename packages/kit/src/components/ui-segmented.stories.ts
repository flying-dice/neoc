import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import { expectHitArea } from '../story-support'
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
  render: () =>
    html`<ui-segmented
      data-testid="seg"
      label="Filter"
      value="all"
      .options=${filters}
    ></ui-segmented>`
}
export const Block: Story = {
  render: () =>
    html`<div style="width:320px">
      <ui-segmented
        data-testid="seg-block"
        label="Filter"
        block
        value="active"
        .options=${filters}
      ></ui-segmented>
    </div>`
}
export const WithDisabledSegment: Story = {
  render: () =>
    html`<ui-segmented
      data-testid="seg-disabled"
      label="Filter"
      value="all"
      .options=${[...filters.slice(0, 2), { ...filters[2]!, disabled: true }]}
    ></ui-segmented>`
}
export const Disabled: Story = {
  render: () =>
    html`<ui-segmented
      data-testid="seg-off"
      label="Filter"
      disabled
      value="all"
      .options=${filters}
    ></ui-segmented>`
}
export const NothingSelected: Story = {
  render: () =>
    html`<ui-segmented data-testid="seg-none" label="Filter" .options=${filters}></ui-segmented>`
}

export const Choosing: Story = {
  render: () =>
    html`<ui-segmented
      data-testid="choose"
      label="Filter"
      value="all"
      .options=${filters}
    ></ui-segmented>`,
  play: async ({ canvasElement }) => {
    const seg = within(canvasElement).getByTestId('choose') as HTMLElement & { value: string }
    // the segments are inside the control's shadow root, where a test id still names them
    const segment = (value: string): HTMLElement =>
      seg.shadowRoot!.querySelector<HTMLElement>(`[data-testid="choose-${value}"]`)!
    const onChange = fn()
    seg.addEventListener('change', onChange)
    await userEvent.click(segment('active'))
    await expect(seg.value).toBe('active')
    await expect(onChange).toHaveBeenLastCalledWith(
      expect.objectContaining({ detail: { value: 'active' } })
    )
    // the arrow keys move the selection and focus together
    await userEvent.keyboard('{ArrowRight}')
    await expect(seg.value).toBe('done')
    await userEvent.keyboard('{ArrowRight}')
    await expect(seg.value).toBe('all')
    await userEvent.keyboard('{End}')
    await expect(seg.value).toBe('done')
    await expect(segment('done')).toHaveAttribute('aria-checked', 'true')
    // the keys left focus (and its ring) inside the shadow root; the story ends with the control at rest
    ;(seg.shadowRoot!.activeElement as HTMLElement | null)?.blur()
  }
}

export const HitAreas: Story = {
  render: () =>
    html`<div style="padding:20px">
      <ui-segmented
        data-testid="hit-seg"
        hit
        label="Filter"
        value="all"
        .options=${filters}
      ></ui-segmented>
    </div>`,
  play: async ({ canvasElement }) => {
    const seg = within(canvasElement).getByTestId('hit-seg')
    await expectHitArea(
      seg.shadowRoot!.querySelector<HTMLElement>('[data-testid="hit-seg-all"]')!,
      seg,
      'vertical'
    )
  }
}
