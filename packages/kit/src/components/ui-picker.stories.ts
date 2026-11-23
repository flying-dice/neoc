import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import { expectHitArea } from '../story-support'
import './ui-picker'

const folders = [
  { value: '1', label: 'Notes' },
  { value: '2', label: 'Work' },
  { value: '3', label: 'Recipes worth making again and again' }
]

const meta: Meta = {
  title: 'Controls/Picker',
  component: 'ui-picker',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

type Picker = HTMLElement & { value: string; updateComplete: Promise<unknown> }
const select = (picker: HTMLElement): HTMLSelectElement =>
  picker.shadowRoot!.querySelector('select')!

export const States: Story = {
  render: () =>
    html`<div style="display:grid;gap:12px;width:240px;justify-items:start">
      <ui-picker data-testid="picker" label="Folder" value="2" .options=${folders}></ui-picker>
      <ui-picker data-testid="picker-long" label="Folder" value="3" .options=${folders}></ui-picker>
      <ui-picker
        data-testid="picker-disabled"
        label="Folder"
        value="1"
        disabled
        .options=${folders}
      ></ui-picker>
      <ui-picker
        data-testid="picker-option-off"
        label="Folder"
        value="1"
        .options=${[folders[0]!, { ...folders[1]!, disabled: true }]}
      ></ui-picker>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await expect(select(canvas.getByTestId('picker')).value).toBe('2')
    // the longest label is cut short inside the width it is given
    await expect(canvas.getByTestId('picker-long').getBoundingClientRect().width).toBe(240)
  }
}

export const Choosing: Story = {
  render: () =>
    html`<form data-testid="form" @submit=${(e: Event) => e.preventDefault()}>
      <ui-picker
        data-testid="choosing"
        name="folder"
        label="Folder"
        value="1"
        .options=${folders}
      ></ui-picker>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const picker = canvas.getByTestId('choosing') as Picker
    const form = canvas.getByTestId('form') as HTMLFormElement
    const onChange = fn()
    picker.addEventListener('change', (e) => onChange((e as CustomEvent).detail))
    await userEvent.selectOptions(select(picker), '2')
    await expect(picker.value).toBe('2')
    await expect(onChange).toHaveBeenCalledTimes(1)
    await expect(onChange).toHaveBeenCalledWith({ value: '2' })
    await expect(new FormData(form).get('folder')).toBe('2')
    // set from outside, the picker shows it and says nothing
    picker.value = '3'
    await picker.updateComplete
    await expect(select(picker).value).toBe('3')
    await expect(onChange).toHaveBeenCalledTimes(1)
  }
}

/** A value no option has shows nothing chosen, and a `required` picker keeps its form invalid until one is. */
export const NothingChosen: Story = {
  render: () =>
    html`<form data-testid="form" @submit=${(e: Event) => e.preventDefault()}>
      <ui-picker
        data-testid="unchosen"
        name="folder"
        label="Folder"
        required
        .options=${folders}
      ></ui-picker>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const picker = canvas.getByTestId('unchosen') as Picker
    const form = canvas.getByTestId('form') as HTMLFormElement
    await expect(select(picker).selectedIndex).toBe(-1)
    await expect(form.checkValidity()).toBe(false)
    await expect(new FormData(form).has('folder')).toBe(false)
    await userEvent.selectOptions(select(picker), '1')
    await expect(form.checkValidity()).toBe(true)
  }
}

export const HitArea: Story = {
  render: () =>
    html`<div style="padding:20px">
      <ui-picker
        data-testid="hit-picker"
        label="Folder"
        value="1"
        hit
        .options=${folders}
      ></ui-picker>
    </div>`,
  play: async ({ canvasElement }) => {
    await expectHitArea(within(canvasElement).getByTestId('hit-picker'))
  }
}
