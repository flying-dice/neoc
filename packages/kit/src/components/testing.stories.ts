import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html } from 'lit'
import { expect, userEvent, within } from 'storybook/test'
import './ui-button'
import './ui-check'
import './ui-list'
import './ui-segmented'
import './ui-text-field'
import './ui-text-area'
import './ui-picker'
import './ui-toggle'

const meta: Meta = {
  title: 'Feedback/Testing Library',
  parameters: { controls: { disable: true } }
}
export default meta

/**
 * The element is the control: a test clicks it by its test id with `userEvent.click(el)` or `user.click(el)` (Testing
 * Library's user-event) and gets what a person's click gets, once, in a stretched column or a grid cell as in an ordinary
 * layout, with the click, `change` and the submit agreeing.
 */
export const ClickingTheElement: StoryObj = {
  render: () =>
    html`<form
      data-testid="tl-form"
      style="display:flex;flex-direction:column;align-items:stretch;width:300px;gap:12px"
      @submit=${(e: Event) => e.preventDefault()}
    >
      <ui-button data-testid="tl-button" type="submit" variant="tinted" size="small"
        >Save</ui-button
      >
      <ui-toggle data-testid="tl-toggle" label="Sync"></ui-toggle>
      <ui-check data-testid="tl-check" label="Done"></ui-check>
      <ui-segmented
        data-testid="tl-seg"
        label="Filter"
        .options=${[
          { value: 'a', label: 'A' },
          { value: 'b', label: 'B' }
        ]}
      ></ui-segmented>
      <ui-text-field data-testid="tl-field" label="Name"></ui-text-field>
      <ui-text-area data-testid="tl-area" label="Notes" rows="2"></ui-text-area>
      <ui-picker
        data-testid="tl-picker"
        label="Folder"
        value="a"
        .options=${[
          { value: 'a', label: 'A' },
          { value: 'b', label: 'B' }
        ]}
      ></ui-picker>
      <ui-list label="Rows">
        <ui-section>
          <ui-row data-testid="tl-row" heading="A row" tappable></ui-row>
        </ui-section>
      </ui-list>
    </form>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const log: string[] = []
    const form = canvas.getByTestId('tl-form')
    form.addEventListener('submit', () => log.push('submit'))
    const button = canvas.getByTestId('tl-button')
    const toggle = canvas.getByTestId('tl-toggle') as HTMLElement & { checked: boolean }
    const check = canvas.getByTestId('tl-check') as HTMLElement & { checked: boolean }
    const row = canvas.getByTestId('tl-row')
    for (const [id, el] of [
      ['button', button],
      ['toggle', toggle],
      ['check', check],
      ['row', row]
    ] as const) {
      for (const type of ['click', 'change', 'press'])
        el.addEventListener(type, () => log.push(`${id}:${type}`))
    }
    const heard = async (act: () => Promise<void>): Promise<string[]> => {
      log.length = 0
      await act()
      return [...log].sort()
    }
    const user = userEvent.setup()
    for (const click of [
      (el: HTMLElement) => userEvent.click(el),
      (el: HTMLElement) => user.click(el)
    ]) {
      await expect(await heard(() => click(button))).toEqual(['button:click', 'submit'])
      toggle.checked = false
      await expect(await heard(() => click(toggle))).toEqual(['toggle:change', 'toggle:click'])
      await expect(toggle.checked).toBe(true)
      check.checked = false
      await expect(await heard(() => click(check))).toEqual(['check:change', 'check:click'])
      await expect(check.checked).toBe(true)
      await expect(await heard(() => click(row))).toEqual(['row:click', 'row:press'])
      // the segmented control, the fields and the picker take a click on the element without error; a field focuses
      // on a real click, and a picker opens the system's menu
      await click(canvas.getByTestId('tl-seg'))
      await click(canvas.getByTestId('tl-field'))
      await click(canvas.getByTestId('tl-area'))
      await click(canvas.getByTestId('tl-picker'))
    }
  }
}
