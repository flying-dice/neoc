import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import { expectHitArea } from '../story-support'
import './ui-text-field'
import './ui-banner'

const meta: Meta = {
  title: 'Controls/Text field',
  component: 'ui-text-field',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

export const States: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:320px">
      <ui-text-field data-testid="field-empty" label="Name" placeholder="Name"></ui-text-field>
      <ui-text-field
        data-testid="field-filled"
        label="Email"
        type="email"
        value="ada@example.com"
      ></ui-text-field>
      <ui-text-field
        data-testid="field-disabled"
        label="Disabled"
        value="Read only here"
        disabled
      ></ui-text-field>
      <ui-text-field data-testid="field-invalid" label="Port" value="99999" invalid></ui-text-field>
    </div>`
}

export const Typing: Story = {
  render: () =>
    html`<form data-testid="form" style="width:320px" @submit=${(e: Event) => e.preventDefault()}>
      <ui-text-field
        data-testid="typing"
        name="title"
        label="Title"
        required
        placeholder="Title"
      ></ui-text-field>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const field = canvas.getByTestId('typing') as HTMLElement & { value: string }
    const form = canvas.getByTestId('form') as HTMLFormElement
    const onInput = fn()
    field.addEventListener('input', onInput)
    await expect(form.checkValidity()).toBe(false) // required and empty
    field.focus() // delegates focus to the input inside
    await userEvent.keyboard('Buy milk')
    await expect(field.value).toBe('Buy milk')
    await expect(onInput).toHaveBeenCalled()
    await expect(form.checkValidity()).toBe(true)
    await expect(new FormData(form).get('title')).toBe('Buy milk')
    // `change` (on leaving the field) is the browser's own and not raised by simulated typing: e2e/kit.spec.ts types for real
  }
}

export const HitArea: Story = {
  render: () =>
    html`<div style="width:320px;padding:20px">
      <ui-text-field data-testid="hit-field" label="Name" placeholder="Name" hit></ui-text-field>
    </div>`,
  play: async ({ canvasElement }) => {
    await expectHitArea(within(canvasElement).getByTestId('hit-field'))
  }
}

export const NumbersAndLimits: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:320px">
      <ui-text-field
        data-testid="port"
        label="Port"
        type="number"
        inputMode="numeric"
        min="1"
        max="65535"
        value="8080"
      ></ui-text-field>
      <ui-text-field
        data-testid="code"
        label="Code"
        maxlength="4"
        pattern="[0-9]+"
        inputMode="numeric"
        placeholder="4 digits"
      ></ui-text-field>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const port = canvas.getByTestId('port') as HTMLElement & {
      value: string
      validity: ValidityState
      checkValidity(): boolean
    }
    await expect(port.shadowRoot!.querySelector('input')).toHaveAttribute('type', 'number')
    await expect(port.checkValidity()).toBe(true)
    port.value = '99999'
    await (port as unknown as { updateComplete: Promise<unknown> }).updateComplete
    await expect(port.validity.rangeOverflow).toBe(true)
  }
}
