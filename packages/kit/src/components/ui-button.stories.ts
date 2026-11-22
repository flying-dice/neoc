import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import { expectHitArea, drawn } from '../story-support'
import './ui-button'
import './ui-icon'

const meta: Meta = {
  title: 'Controls/Button',
  component: 'ui-button',
  tags: ['autodocs'],
  args: {
    variant: 'filled',
    size: 'medium',
    shape: 'rounded',
    destructive: false,
    disabled: false
  },
  argTypes: {
    variant: { control: 'inline-radio', options: ['filled', 'tinted', 'gray', 'plain'] },
    size: { control: 'inline-radio', options: ['small', 'medium', 'large'] },
    shape: { control: 'inline-radio', options: ['rounded', 'capsule'] }
  },
  render: (a) =>
    html`<ui-button
      data-testid="button"
      variant=${a.variant}
      size=${a.size}
      shape=${a.shape}
      ?destructive=${a.destructive}
      ?disabled=${a.disabled}
      >Continue</ui-button
    >`
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
  render: () =>
    html`<div style="display:grid;grid-template-columns:repeat(4,auto);gap:12px;align-items:center">
      ${(['small', 'medium', 'large'] as const).map((size) =>
        (['filled', 'tinted', 'gray', 'plain'] as const).map(
          (variant) =>
            html`<ui-button data-testid="button-${size}-${variant}" variant=${variant} size=${size}
              >${variant}</ui-button
            >`
        )
      )}
    </div>`
}

export const WithIcon: Story = {
  parameters: { controls: { disable: true } },
  render: () =>
    html`<div style="display:flex;gap:12px">
      <ui-button data-testid="button-new"
        ><ui-icon slot="icon" name="plus" size="17" weight="2.2"></ui-icon>New Agent</ui-button
      >
      <ui-button data-testid="button-run" variant="tinted" shape="capsule"
        ><ui-icon slot="icon" name="play" size="15"></ui-icon>Run</ui-button
      >
      <ui-button data-testid="button-more" variant="gray" square label="More"
        ><ui-icon name="ellipsis" size="19"></ui-icon
      ></ui-button>
    </div>`
}

export const DataColourFill: Story = {
  parameters: { controls: { disable: true } },
  render: () =>
    html`<div style="display:flex;flex-wrap:wrap;gap:12px;max-width:420px">
      ${[
        'blue',
        'red',
        'green',
        'orange',
        'yellow',
        'purple',
        'pink',
        'teal',
        'mint',
        'gray',
        '#1d3557',
        '#f1faee'
      ].map(
        (fill) =>
          html`<ui-button data-testid="button-fill-${fill.replace('#', '')}" fill=${fill}
            >${fill}</ui-button
          >`
      )}
    </div>`
}

export const PressedAndRound: Story = {
  parameters: { controls: { disable: true } },
  render: () =>
    html`<div
      style="display:grid;grid-template-columns:repeat(3,64px);grid-auto-rows:64px;gap:10px"
    >
      <ui-button data-testid="key-7" shape="circle" size="fill" fill="gray4">7</ui-button>
      <ui-button data-testid="key-add" shape="circle" size="fill" fill="orange">+</ui-button>
      <ui-button
        data-testid="key-sub"
        shape="circle"
        size="fill"
        fill="orange"
        toggle
        pressed
        label="Minus"
        >−</ui-button
      >
    </div>`
}

export const Pressing: Story = {
  parameters: { controls: { disable: true } },
  render: () =>
    html`<ui-button data-testid="press" variant="tinted" id="press">Press me</ui-button>`,
  play: async ({ canvasElement }) => {
    const onClick = fn()
    const button = within(canvasElement).getByTestId('press')
    button.addEventListener('click', onClick)
    await userEvent.click(drawn(button))
    await expect(onClick).toHaveBeenCalledTimes(1)
    // the keyboard presses it too: focus it, then Space and Enter
    button.focus()
    await userEvent.keyboard(' ')
    await userEvent.keyboard('{Enter}')
    await expect(onClick).toHaveBeenCalledTimes(3)
  }
}

/**
 * `hit` is the shell's: a 44 x 44 pt box around the drawn button, off by default (the button is drawn at its own size, so
 * dense layouts such as the shell's All Variants grid are reproduced). The drawn size is the same either way.
 */
export const HitAreas: Story = {
  parameters: { controls: { disable: true } },
  render: () =>
    html`<div
      style="display:grid;grid-template-columns:auto auto;gap:40px;padding:20px;justify-items:start;align-items:start"
    >
      <ui-button data-testid="plain-small" size="small">Small</ui-button>
      <ui-button data-testid="hit-small" size="small" hit>Small, hit</ui-button>
      <ui-button data-testid="plain-medium">Medium</ui-button>
      <ui-button data-testid="hit-medium" hit>Medium, hit</ui-button>
      <ui-button data-testid="plain-square" square size="small" label="Add"
        ><ui-icon name="plus" size="15"></ui-icon
      ></ui-button>
      <ui-button data-testid="hit-square" square size="small" hit label="Add"
        ><ui-icon name="plus" size="15"></ui-icon
      ></ui-button>
      <ui-button data-testid="hit-plain" variant="plain" size="small" hit>Plain, hit</ui-button>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const box = (id: string): DOMRect => canvas.getByTestId(id).getBoundingClientRect()
    // off: the element is the drawn button (28 and 34 pt high, 28 wide for a small square one)
    await expect(box('plain-small').height).toBe(28)
    await expect(box('plain-medium').height).toBe(34)
    await expect(box('plain-square').width).toBe(28)
    // on: a 44 pt box, and the same drawn button inside it
    for (const id of ['hit-small', 'hit-medium', 'hit-square', 'hit-plain']) {
      await expect(box(id).height).toBe(44)
      await expectHitArea(canvas.getByTestId(id))
    }
    await expect(box('hit-square').width).toBe(44)
    const drawn = (id: string): DOMRect =>
      canvas.getByTestId(id).shadowRoot!.querySelector('button')!.getBoundingClientRect()
    await expect(drawn('hit-small').height).toBe(28)
    await expect(drawn('hit-medium').height).toBe(34)
  }
}
