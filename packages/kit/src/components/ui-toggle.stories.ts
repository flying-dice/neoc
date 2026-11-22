import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'
import { expectHitArea } from '../story-support'
import './ui-toggle'
import './ui-check'

const meta: Meta = {
  title: 'Controls/Toggle and Check',
  component: 'ui-toggle',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

export const States: Story = {
  render: () =>
    html`<div
      style="display:grid;grid-template-columns:auto auto auto auto;gap:16px 28px;align-items:center"
    >
      <ui-toggle data-testid="toggle-off" label="Wi-Fi off"></ui-toggle>
      <ui-toggle data-testid="toggle-on" label="Wi-Fi on" checked></ui-toggle>
      <ui-toggle data-testid="toggle-disabled" label="Disabled off" disabled></ui-toggle>
      <ui-toggle data-testid="toggle-disabled-on" label="Disabled on" disabled checked></ui-toggle>
      <ui-check data-testid="check-off" label="Done off"></ui-check>
      <ui-check data-testid="check-on" label="Done on" checked></ui-check>
      <ui-check data-testid="check-disabled" label="Disabled" disabled></ui-check>
      <ui-toggle
        data-testid="toggle-tint"
        label="Tinted"
        checked
        tint="var(--ios-orange)"
      ></ui-toggle>
    </div>`
}

export const Flipping: Story = {
  render: () =>
    html`<div style="display:flex;gap:24px">
      <ui-toggle data-testid="flip-toggle" label="Notifications"></ui-toggle>
      <ui-check data-testid="flip-check" label="Done"></ui-check>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    for (const id of ['flip-toggle', 'flip-check']) {
      const el = canvas.getByTestId(id) as HTMLElement & { checked: boolean }
      const onChange = fn()
      el.addEventListener('change', onChange)
      await userEvent.click(el)
      await expect(el.checked).toBe(true)
      await expect(onChange).toHaveBeenLastCalledWith(
        expect.objectContaining({ detail: { checked: true } })
      )
      el.focus()
      await userEvent.keyboard(' ')
      await expect(el.checked).toBe(false)
      await userEvent.keyboard('{Enter}')
      await expect(el.checked).toBe(true)
      await expect(onChange).toHaveBeenCalledTimes(3)
    }
  }
}

export const InAForm: Story = {
  render: () =>
    html`<form
      data-testid="form"
      style="display:flex;gap:16px;align-items:center"
      @submit=${(e: Event) => e.preventDefault()}
    >
      <label for="f-toggle">Sync</label>
      <ui-toggle id="f-toggle" data-testid="form-toggle" name="sync"></ui-toggle>
      <ui-check data-testid="form-check" name="done" value="yes" label="Done"></ui-check>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const form = canvas.getByTestId('form') as HTMLFormElement
    const toggle = canvas.getByTestId('form-toggle') as HTMLElement & { checked: boolean }
    await expect(new FormData(form).get('sync')).toBeNull()
    // a click on the <label> flips the toggle it names
    canvas.getByText('Sync').click()
    await expect(toggle.checked).toBe(true)
    await userEvent.click(canvas.getByTestId('form-check'))
    const data = new FormData(form)
    await expect(data.get('sync')).toBe('on')
    await expect(data.get('done')).toBe('yes')
    form.reset()
    await expect(toggle.checked).toBe(false)
  }
}

export const HitAreas: Story = {
  render: () =>
    html`<div style="display:grid;gap:40px;padding:20px;justify-items:start">
      <ui-toggle data-testid="hit-toggle" label="Wi-Fi"></ui-toggle>
      <ui-check data-testid="hit-check" label="Done"></ui-check>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    for (const id of ['hit-toggle', 'hit-check']) await expectHitArea(canvas.getByTestId(id))
  }
}

export const RequiredCheck: Story = {
  render: () =>
    html`<form
      data-testid="agree-form"
      @submit=${(e: Event) => e.preventDefault()}
      style="display:flex;gap:12px;align-items:center"
    >
      <ui-check id="agree" data-testid="agree" name="agree" required label="I agree"></ui-check>
      <label for="agree">I agree to the terms</label>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const form = canvas.getByTestId('agree-form') as HTMLFormElement
    const check = canvas.getByTestId('agree') as HTMLElement & {
      checked: boolean
      validity: ValidityState
    }
    await expect(form.checkValidity()).toBe(false)
    await expect(check.validity.valueMissing).toBe(true)
    canvas.getByText('I agree to the terms').click()
    await expect(check.checked).toBe(true)
    await expect(form.checkValidity()).toBe(true)
  }
}
