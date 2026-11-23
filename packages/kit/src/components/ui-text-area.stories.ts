import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import './ui-text-area'

const meta: Meta = {
  title: 'Controls/Text area',
  component: 'ui-text-area',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

const long = Array.from({ length: 12 }, (_, i) => `Line ${i + 1} of a note that goes on.`).join(
  '\n'
)

export const States: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:320px">
      <ui-text-area data-testid="area-empty" label="Notes" placeholder="Notes"></ui-text-area>
      <ui-text-area
        data-testid="area-filled"
        label="Description"
        value=${'The tap in the kitchen drips.\nIt wants a new washer.'}
      ></ui-text-area>
      <ui-text-area
        data-testid="area-disabled"
        label="Disabled"
        value="Not to be changed here"
        disabled
      ></ui-text-area>
      <ui-text-area
        data-testid="area-invalid"
        label="Reason"
        value="Too short"
        invalid
      ></ui-text-area>
      <ui-text-area
        data-testid="area-readonly"
        label="Read only"
        value="Can be selected and copied"
        readonly
      ></ui-text-area>
    </div>`
}

/** More text than its rows scrolls in the field; a bare one given a height fills it. */
export const LongTextAndBare: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:320px">
      <ui-text-area data-testid="area-long" label="Long" rows="4" .value=${long}></ui-text-area>
      <div style="height:160px;display:flex;box-shadow:0 0 0 1px var(--separator)">
        <ui-text-area
          data-testid="area-bare"
          style="flex:1"
          variant="bare"
          label="Note"
          placeholder="Start writing"
        ></ui-text-area>
      </div>
    </div>`,
  play: async ({ canvasElement }) => {
    const bare = within(canvasElement).getByTestId('area-bare')
    await expect(bare.shadowRoot!.querySelector('textarea')!.getBoundingClientRect().height).toBe(
      160
    )
  }
}

export const Typing: Story = {
  render: () =>
    html`<form data-testid="form" style="width:320px" @submit=${(e: Event) => e.preventDefault()}>
      <ui-text-area
        data-testid="typing"
        name="notes"
        label="Notes"
        required
        placeholder="Notes"
      ></ui-text-area>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const area = canvas.getByTestId('typing') as HTMLElement & { value: string }
    const form = canvas.getByTestId('form') as HTMLFormElement
    const onInput = fn()
    const onSubmit = fn()
    area.addEventListener('input', onInput)
    form.addEventListener('submit', onSubmit)
    await expect(form.checkValidity()).toBe(false) // required and empty
    area.focus() // delegates focus to the textarea inside
    await userEvent.keyboard('Buy milk{Enter}and bread')
    await expect(area.value).toBe('Buy milk\nand bread')
    await expect(onInput).toHaveBeenCalled()
    await expect(onSubmit).not.toHaveBeenCalled() // Return is a new line
    await expect(form.checkValidity()).toBe(true)
    await expect(new FormData(form).get('notes')).toBe('Buy milk\nand bread')
  }
}
