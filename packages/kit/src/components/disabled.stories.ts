import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html, type TemplateResult } from 'lit'
import { expect, userEvent, within } from 'storybook/test'
import './ui-button'
import './ui-check'
import './ui-list'
import './ui-segmented'
import './ui-text-field'
import './ui-toggle'

const meta: Meta = {
  title: 'Feedback/Disabled',
  parameters: { controls: { disable: true }, layout: 'padded' }
}
export default meta

const options = [
  { value: 'a', label: 'A' },
  { value: 'b', label: 'B' }
]

/** Each control, disabled by its own attribute or by a disabled fieldset, in a clickable card. */
const controls = (disabled: 'own' | 'fieldset'): TemplateResult => {
  const own = disabled === 'own'
  return html`<fieldset
    ?disabled=${!own}
    style="border:0;margin:0;padding:0;min-width:0;display:grid;gap:12px;width:360px"
  >
    <div data-testid="${disabled}-card-native" style="padding:8px;background:var(--fill-tertiary)">
      <button data-testid="${disabled}-native" type="button" ?disabled=${own}>Native</button>
    </div>
    <div data-testid="${disabled}-card-button" style="padding:8px;background:var(--fill-tertiary)">
      <ui-button data-testid="${disabled}-button" variant="tinted" ?disabled=${own}
        >Button</ui-button
      >
    </div>
    <div data-testid="${disabled}-card-toggle" style="padding:8px;background:var(--fill-tertiary)">
      <ui-toggle data-testid="${disabled}-toggle" label="Toggle" ?disabled=${own}></ui-toggle>
    </div>
    <div data-testid="${disabled}-card-check" style="padding:8px;background:var(--fill-tertiary)">
      <ui-check data-testid="${disabled}-check" label="Check" ?disabled=${own}></ui-check>
    </div>
    <div data-testid="${disabled}-card-seg" style="padding:8px;background:var(--fill-tertiary)">
      <ui-segmented
        data-testid="${disabled}-seg"
        label="Seg"
        .options=${options}
        ?disabled=${own}
      ></ui-segmented>
    </div>
    <div data-testid="${disabled}-card-field" style="padding:8px;background:var(--fill-tertiary)">
      <ui-text-field data-testid="${disabled}-field" label="Field" ?disabled=${own}></ui-text-field>
    </div>
  </fieldset>`
}

/**
 * Disabled means one thing: the control hears nothing, activates nothing, takes no focus, and swallows the click so that
 * the parent hears nothing either, as the native disabled button (the control experiment) does. A simulated click from
 * Testing Library (`userEvent.click`, `user.click`) gets the same answer as a person's.
 */
export const DisabledControls: StoryObj = {
  render: () =>
    html`<div style="display:flex;gap:32px">${controls('own')}${controls('fieldset')}</div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const log: string[] = []
    const kinds = ['native', 'button', 'toggle', 'check', 'seg', 'field']
    for (const way of ['own', 'fieldset']) {
      for (const kind of kinds) {
        const id = `${way}-${kind}`
        for (const type of ['click', 'change', 'focusin', 'press'])
          canvas.getByTestId(id).addEventListener(type, () => log.push(`${id}:${type}`))
        canvas
          .getByTestId(`${way}-card-${kind}`)
          .addEventListener('click', () => log.push(`${id}:card`))
      }
    }
    const user = userEvent.setup()
    const calls: [string, (el: HTMLElement) => Promise<void>][] = [
      ['userEvent.click', (el) => userEvent.click(el)],
      ['user.click', (el) => user.click(el)],
      ['el.click()', async (el) => el.click()]
    ]
    for (const [name, click] of calls) {
      for (const way of ['own', 'fieldset']) {
        for (const kind of kinds) {
          log.length = 0
          await click(canvas.getByTestId(`${way}-${kind}`))
          await expect(log, `${name} on the ${way} disabled ${kind}`).toEqual([])
        }
      }
    }
    // and none of them takes focus
    for (const way of ['own', 'fieldset'])
      for (const kind of kinds) {
        const el = canvas.getByTestId(`${way}-${kind}`)
        el.focus()
        await expect(
          document.activeElement === el ||
            (document.activeElement !== document.body && el.contains(document.activeElement)),
          `${way} ${kind} took focus`
        ).toBe(false)
      }
  }
}
