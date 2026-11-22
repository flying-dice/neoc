import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
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
  render: () => html`<div style="display:grid;gap:12px;width:320px">
    <ui-text-field data-testid="field-empty" label="Name" placeholder="Name"></ui-text-field>
    <ui-text-field data-testid="field-filled" label="Email" type="email" value="ada@example.com"></ui-text-field>
    <ui-text-field data-testid="field-disabled" label="Disabled" value="Read only here" disabled></ui-text-field>
    <ui-text-field data-testid="field-invalid" label="Port" value="99999" invalid></ui-text-field>
  </div>`
}

export const Typing: Story = {
  render: () => html`<form data-testid="form" style="width:320px" @submit=${(e: Event) => e.preventDefault()}>
    <ui-text-field data-testid="typing" name="title" label="Title" required placeholder="Title"></ui-text-field>
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
