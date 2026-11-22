import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import './ui-button'
import './ui-icon'

const meta: Meta = {
  title: 'Controls/Button',
  component: 'ui-button',
  tags: ['autodocs'],
  args: { variant: 'filled', size: 'medium', shape: 'rounded', destructive: false, disabled: false },
  argTypes: {
    variant: { control: 'inline-radio', options: ['filled', 'tinted', 'gray', 'plain'] },
    size: { control: 'inline-radio', options: ['small', 'medium', 'large'] },
    shape: { control: 'inline-radio', options: ['rounded', 'capsule'] }
  },
  render: (a) =>
    html`<ui-button data-testid="button" variant=${a.variant} size=${a.size} shape=${a.shape} ?destructive=${a.destructive} ?disabled=${a.disabled}>Continue</ui-button>`
}
export default meta
type Story = StoryObj

export const Filled: Story = {}
export const Tinted: Story = { args: { variant: 'tinted' } }
export const Gray: Story = { args: { variant: 'gray' } }
export const Plain: Story = { args: { variant: 'plain' } }
export const Capsule: Story = { args: { shape: 'capsule' } }
export const Destructive: Story = { args: { variant: 'tinted', destructive: true } }
export const Disabled: Story = { args: { disabled: true } }

export const AllVariants: Story = {
  parameters: { controls: { disable: true } },
  render: () => html`<div style="display:grid;grid-template-columns:repeat(4,auto);gap:12px;align-items:center">
    ${(['small', 'medium', 'large'] as const).map((size) =>
      (['filled', 'tinted', 'gray', 'plain'] as const).map(
        (variant) => html`<ui-button data-testid="button-${size}-${variant}" variant=${variant} size=${size}>${variant}</ui-button>`
      )
    )}
  </div>`
}

export const WithIcon: Story = {
  parameters: { controls: { disable: true } },
  render: () => html`<div style="display:flex;gap:12px">
    <ui-button data-testid="button-new"><ui-icon slot="icon" name="plus" size="17" weight="2.2"></ui-icon>New Agent</ui-button>
    <ui-button data-testid="button-run" variant="tinted" shape="capsule"><ui-icon slot="icon" name="play" size="15"></ui-icon>Run</ui-button>
    <ui-button data-testid="button-more" variant="gray" square label="More"><ui-icon name="ellipsis" size="19"></ui-icon></ui-button>
  </div>`
}

export const DataColourFill: Story = {
  parameters: { controls: { disable: true } },
  render: () => html`<div style="display:flex;flex-wrap:wrap;gap:12px;max-width:420px">
    ${['blue', 'red', 'green', 'orange', 'yellow', 'purple', 'pink', 'teal', 'mint', 'gray', '#1d3557', '#f1faee'].map(
      (fill) => html`<ui-button data-testid="button-fill-${fill.replace('#', '')}" fill=${fill}>${fill}</ui-button>`
    )}
  </div>`
}

export const PressedAndRound: Story = {
  parameters: { controls: { disable: true } },
  render: () => html`<div style="display:flex;gap:10px;width:260px;height:64px">
    <ui-button data-testid="key-7" shape="circle" size="fill" fill="gray4">7</ui-button>
    <ui-button data-testid="key-add" shape="circle" size="fill" fill="orange">+</ui-button>
    <ui-button data-testid="key-sub" shape="circle" size="fill" fill="orange" toggle pressed label="Minus">−</ui-button>
  </div>`
}

export const Pressing: Story = {
  parameters: { controls: { disable: true } },
  render: () => html`<ui-button data-testid="press" variant="tinted" id="press">Press me</ui-button>`,
  play: async ({ canvasElement }) => {
    const onClick = fn()
    const button = within(canvasElement).getByTestId('press')
    button.addEventListener('click', onClick)
    await userEvent.click(button)
    await expect(onClick).toHaveBeenCalledTimes(1)
    // the keyboard presses it too: Tab to it, then Space and Enter
    await userEvent.tab()
    await userEvent.keyboard(' ')
    await userEvent.keyboard('{Enter}')
    await expect(onClick).toHaveBeenCalledTimes(3)
  }
}
