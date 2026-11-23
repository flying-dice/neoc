import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html, type TemplateResult } from 'lit'
import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
import './ui-alert'
import './ui-button'

const meta: Meta = {
  title: 'Feedback/Alert',
  component: 'ui-alert',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

type Alert = HTMLElement & { open: boolean; updateComplete: Promise<unknown> }
const answer = (alert: HTMLElement, part: 'cancel' | 'action'): HTMLElement =>
  alert.shadowRoot!.querySelector<HTMLElement>(`[part=${part}]`)!
const focused = (alert: HTMLElement): Element | null => alert.shadowRoot!.activeElement

/** The page an alert is asked from: a button that opens it, and the alert beside it. */
const asking = (alert: TemplateResult): TemplateResult =>
  html`<div style="min-width:320px;min-height:260px;display:grid;place-items:center">
    <ui-button
      data-testid="ask"
      variant="plain"
      destructive
      hit
      @click=${(e: Event) => {
        ;((e.currentTarget as HTMLElement).nextElementSibling as Alert).open = true
      }}
      >Delete List</ui-button
    >
    ${alert}
  </div>`

export const Destructive: Story = {
  render: () =>
    asking(
      html`<ui-alert
        data-testid="alert"
        open
        destructive
        heading="Delete Work?"
        message="Its todos are deleted with it."
        action-label="Delete"
      ></ui-alert>`
    ),
  play: async ({ canvasElement }) => {
    const alert = within(canvasElement).getByTestId('alert')
    const dialog = alert.shadowRoot!.querySelector('dialog')!
    await expect(dialog).toHaveAttribute('role', 'alertdialog')
    await expect(dialog.matches(':modal')).toBe(true)
    // the answer that deletes nothing is the one in front
    await waitFor(() => expect(focused(alert)).toBe(answer(alert, 'cancel')))
  }
}

export const NotDestructive: Story = {
  render: () =>
    asking(
      html`<ui-alert
        data-testid="alert"
        open
        heading="Restart the server?"
        message="It is away for a few seconds."
        action-label="Restart"
      ></ui-alert>`
    ),
  play: async ({ canvasElement }) => {
    const alert = within(canvasElement).getByTestId('alert')
    await waitFor(() => expect(focused(alert)).toBe(answer(alert, 'action')))
  }
}

export const LongText: Story = {
  render: () =>
    asking(
      html`<ui-alert
        data-testid="alert"
        open
        destructive
        heading="Delete the notebook Things-I-mean-to-read-one-day-and-probably-never-will?"
        message="Its 214 notes, the links between them and every tag only they carry are deleted with it. Nothing else in the other notebooks changes, and none of it can be brought back afterwards."
        action-label="Delete"
      ></ui-alert>`
    )
}

/** Opened from a button: Escape answers Cancel, the alert closes and the focus goes back to the button. */
export const EscapeCancels: Story = {
  tags: ['play-only'],
  render: () =>
    asking(
      html`<ui-alert
        data-testid="alert"
        destructive
        heading="Delete Work?"
        message="Its todos are deleted with it."
        action-label="Delete"
      ></ui-alert>`
    ),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const alert = canvas.getByTestId('alert') as Alert
    const ask = canvas.getByTestId('ask')
    const oncancel = fn()
    const onconfirm = fn()
    alert.addEventListener('cancel', oncancel)
    alert.addEventListener('confirm', onconfirm)
    const dialog = alert.shadowRoot!.querySelector('dialog')!
    await expect(dialog.open).toBe(false)

    ask.focus()
    await userEvent.keyboard('{Enter}')
    await waitFor(() => expect(focused(alert)).toBe(answer(alert, 'cancel')))
    // a modal dialog: the browser keeps the focus inside and the page behind inert (e2e/kit.spec.ts tabs for real)
    await expect(dialog.matches(':modal')).toBe(true)

    await userEvent.keyboard('{Escape}')
    await expect(oncancel).toHaveBeenCalledTimes(1)
    await expect(onconfirm).not.toHaveBeenCalled()
    await expect(alert.open).toBe(false)
    await waitFor(() => expect(dialog.open).toBe(false))
    await waitFor(() => expect(ask.matches(':focus-within')).toBe(true))
  }
}

export const AnsweringGoesAhead: Story = {
  tags: ['play-only'],
  render: () =>
    asking(
      html`<ui-alert
        data-testid="alert"
        open
        destructive
        heading="Delete Work?"
        action-label="Delete"
      ></ui-alert>`
    ),
  play: async ({ canvasElement }) => {
    const alert = within(canvasElement).getByTestId('alert') as Alert
    const oncancel = fn()
    const onconfirm = fn()
    alert.addEventListener('cancel', oncancel)
    alert.addEventListener('confirm', onconfirm)
    await waitFor(() => expect(focused(alert)).toBe(answer(alert, 'cancel')))
    await userEvent.click(answer(alert, 'action'))
    await expect(onconfirm).toHaveBeenCalledTimes(1)
    await expect(oncancel).not.toHaveBeenCalled()
    await expect(alert.open).toBe(false)
    // opened again, Cancel answers once
    alert.open = true
    await alert.updateComplete
    await userEvent.click(answer(alert, 'cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
    await expect(onconfirm).toHaveBeenCalledTimes(1)
  }
}
