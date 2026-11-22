/**
 * A kit control in a form behaves as its native counterpart does, however its properties arrive: as attributes (plain
 * HTML), or as properties (React 19 and Svelte 5 create the element and set properties; Lit's `.name=` does the same),
 * before it is connected or after. One checklist, run against each way:
 *
 *   value in FormData, disabled by itself and by a fieldset, required and validity (and the app's own message), reset,
 *   restore, labels, submit by Enter and by a kit button.
 *
 * It runs in the embedded frame: sandboxed, opaque origin, the frame's policy, with `allow-forms` as the host sets it.
 */
import { startFrameServer, type FrameServer } from './frame-server'
import type { Page } from '@playwright/test'
import { expect, test } from '../../../e2e/fixtures'

let server: FrameServer

test.beforeAll(async () => {
  server = await startFrameServer()
})
test.afterAll(() => server.close())

type Way = 'attributes' | 'properties before connecting' | 'properties after connecting'
const WAYS: Way[] = ['attributes', 'properties before connecting', 'properties after connecting']

/** Builds the same form three ways (in the page: the browser's own custom element machinery is the thing under test). */
const build = (page: Page, way: Way): Promise<void> =>
  page.evaluate(async (way) => {
    document.body.replaceChildren()
    const form = document.createElement('form')
    form.id = 'f'
    form.addEventListener('submit', (e) => {
      e.preventDefault()
      const w = window as unknown as {
        submits: { entries: [string, string][]; submitter: string | null }[]
      }
      w.submits.push({
        entries: [...new FormData(form)] as [string, string][],
        submitter: e.submitter?.tagName ?? null
      })
    })
    ;(window as unknown as { submits: unknown[] }).submits = []
    document.body.append(form)

    const specs: [string, Record<string, unknown>, string?][] = [
      ['ui-toggle', { name: 'sync', checked: true, id: 'toggle', label: 'Sync' }],
      [
        'ui-check',
        { name: 'agree', value: 'yes', checked: true, required: true, id: 'check', label: 'Agree' }
      ],
      [
        'ui-segmented',
        {
          name: 'size',
          value: 'b',
          id: 'seg',
          label: 'Size',
          options: [
            { value: 'a', label: 'A' },
            { value: 'b', label: 'B' }
          ]
        }
      ],
      ['ui-text-field', { name: 'who', value: 'Ada', id: 'who', required: true }],
      ['ui-button', { name: 'go', value: '1', type: 'submit', id: 'go' }, 'Go']
    ]
    for (const [tag, props, text] of specs) {
      const el = document.createElement(tag)
      if (text) el.textContent = text
      const set = (): void => {
        for (const [k, v] of Object.entries(props)) {
          if (way === 'attributes' && k !== 'options') {
            if (v === true) el.setAttribute(k, '')
            else el.setAttribute(k, String(v))
          } else (el as unknown as Record<string, unknown>)[k] = v
        }
      }
      if (way === 'properties after connecting') {
        form.append(el)
        set()
      } else {
        set()
        form.append(el)
      }
    }
    const label = document.createElement('label')
    label.htmlFor = 'who'
    label.textContent = 'Who'
    form.prepend(label)
    await Promise.all(
      [...form.children]
        .filter((c) => 'updateComplete' in c)
        .map((c) => (c as unknown as { updateComplete: Promise<unknown> }).updateComplete)
    )
  }, way)

const entries = (page: Page): Promise<[string, string][]> =>
  page.evaluate(
    () => [...new FormData(document.getElementById('f') as HTMLFormElement)] as [string, string][]
  )

for (const way of WAYS) {
  test.describe(`controls set as ${way}`, () => {
    test.beforeEach(async ({ page }) => {
      await page.goto(`${server.origin}/frame`)
      await build(page, way)
    })

    test('put their value in the form data (the button only when it submits)', async ({ page }) => {
      expect(await entries(page)).toEqual([
        ['sync', 'on'],
        ['agree', 'yes'],
        ['size', 'b'],
        ['who', 'Ada']
      ])
    })

    test('are disabled by themselves and by a disabled fieldset, and then submit nothing', async ({
      page
    }) => {
      const result = await page.evaluate(async () => {
        const form = document.getElementById('f') as HTMLFormElement
        const fieldset = document.createElement('fieldset')
        fieldset.disabled = true
        fieldset.append(
          ...form.querySelectorAll(
            ':scope > ui-toggle, :scope > ui-check, :scope > ui-segmented, :scope > ui-text-field, :scope > ui-button'
          )
        )
        form.append(fieldset)
        await new Promise((r) => setTimeout(r, 50))
        const inner = (id: string, sel: string): boolean =>
          (
            document.getElementById(id)!.shadowRoot!.querySelector(sel) as
              HTMLButtonElement | HTMLInputElement
          ).disabled
        return {
          entries: [...new FormData(form)],
          toggle: inner('toggle', 'button'),
          check: inner('check', 'button'),
          seg: (
            document
              .getElementById('seg')!
              .shadowRoot!.querySelector('[role=radio]') as HTMLButtonElement
          ).disabled,
          field: inner('who', 'input'),
          button: inner('go', 'button')
        }
      })
      expect(result).toEqual({
        entries: [],
        toggle: true,
        check: true,
        seg: true,
        field: true,
        button: true
      })
      // a click on the disabled submit button submits nothing
      await page.evaluate(() =>
        (
          document.getElementById('go')!.shadowRoot!.querySelector('button') as HTMLButtonElement
        ).click()
      )
      expect(
        await page.evaluate(() => (window as unknown as { submits: unknown[] }).submits.length)
      ).toBe(0)
    })

    test('are required, and say so through validity; the app can set its own message', async ({
      page
    }) => {
      const result = await page.evaluate(async () => {
        const form = document.getElementById('f') as HTMLFormElement
        const q = (
          id: string
        ): HTMLElement & {
          checked: boolean
          value: string
          validity: ValidityState
          validationMessage: string
          checkValidity(): boolean
          setCustomValidity(m: string): void
          willValidate: boolean
          form: HTMLFormElement | null
          labels: NodeList
          updateComplete: Promise<unknown>
        } => document.getElementById(id) as never
        const out: Record<string, unknown> = {}
        out.valid = form.checkValidity()
        q('check').checked = false
        q('who').value = ''
        await Promise.all([q('check').updateComplete, q('who').updateComplete])
        out.invalid = form.checkValidity()
        out.checkMissing = q('check').validity.valueMissing
        out.checkMessage = q('check').validationMessage
        out.fieldMissing = q('who').validity.valueMissing
        out.checkWillValidate = q('check').willValidate
        out.checkForm = q('check').form === form
        out.whoLabels = q('who').labels.length
        out.whoName = q('who').shadowRoot!.querySelector('input')!.getAttribute('aria-label')
        q('check').checked = true
        q('who').value = 'Ada'
        await Promise.all([q('check').updateComplete, q('who').updateComplete])
        out.fixed = form.checkValidity()
        q('who').setCustomValidity('Already taken')
        out.customInvalid = [
          form.checkValidity(),
          q('who').validity.customError,
          q('who').validationMessage
        ]
        q('who').setCustomValidity('')
        out.customCleared = form.checkValidity()
        return out
      })
      expect(result).toEqual({
        valid: true,
        invalid: false,
        checkMissing: true,
        checkMessage: expect.any(String),
        fieldMissing: true,
        checkWillValidate: true,
        checkForm: true,
        whoLabels: 1,
        whoName: 'Who',
        fixed: true,
        customInvalid: [false, true, 'Already taken'],
        customCleared: true
      })
    })

    test('reset puts them back, and a restored state puts the value back', async ({ page }) => {
      const result = await page.evaluate(async () => {
        const form = document.getElementById('f') as HTMLFormElement
        const q = (
          id: string
        ): HTMLElement &
          Record<string, unknown> & {
            updateComplete: Promise<unknown>
            formStateRestoreCallback(s: string | null): void
          } => document.getElementById(id) as never
        q('toggle').checked = false
        q('check').checked = false
        q('seg').value = 'a'
        q('who').value = 'Grace'
        await Promise.all(['toggle', 'check', 'seg', 'who'].map((id) => q(id).updateComplete))
        form.reset()
        await Promise.all(['toggle', 'check', 'seg', 'who'].map((id) => q(id).updateComplete))
        const afterReset = [...new FormData(form)]
        // the browser gives back what it kept: the control's value while it was on, a string for the others
        q('check').checked = false
        q('check').formStateRestoreCallback('yes')
        q('seg').formStateRestoreCallback('a')
        q('who').formStateRestoreCallback('Linus')
        await Promise.all(['check', 'seg', 'who'].map((id) => q(id).updateComplete))
        return { afterReset, restored: [q('check').checked, q('seg').value, q('who').value] }
      })
      expect(result.afterReset).toEqual([
        ['sync', 'on'],
        ['agree', 'yes'],
        ['size', 'b'],
        ['who', 'Ada']
      ])
      expect(result.restored).toEqual([true, 'a', 'Linus'])
    })

    test('submit by Enter in the field, and by a kit button, with the right data', async ({
      page
    }) => {
      await page.locator('#who').focus()
      await page.keyboard.press('Enter')
      await page.locator('#go').click()
      const submits = await page.evaluate(
        () =>
          (
            window as unknown as {
              submits: { entries: [string, string][]; submitter: string | null }[]
            }
          ).submits
      )
      expect(submits).toHaveLength(2)
      // Enter: the data of the form; the kit button: the same plus its own name and value
      expect(submits[0]!.entries).toEqual([
        ['sync', 'on'],
        ['agree', 'yes'],
        ['size', 'b'],
        ['who', 'Ada']
      ])
      expect(submits[1]!.entries).toEqual([
        ['sync', 'on'],
        ['agree', 'yes'],
        ['size', 'b'],
        ['who', 'Ada'],
        ['go', '1']
      ])
      // a kit button cannot be the `submitter` (only a native submit button can): documented in the README
      expect(submits[1]!.submitter).toBeNull()
      // and the button's entry is not left behind for the next submission
      await page.locator('#who').focus()
      await page.keyboard.press('Enter')
      const third = await page.evaluate(
        () =>
          (window as unknown as { submits: { entries: [string, string][] }[] }).submits[2]!.entries
      )
      expect(third.some(([k]) => k === 'go')).toBe(false)
    })
  })
}

test('a text field takes the attributes of an ordinary input, including a number', async ({
  page
}) => {
  await page.goto(`${server.origin}/frame`)
  const result = await page.evaluate(async () => {
    const field = document.createElement('ui-text-field') as HTMLElement &
      Record<string, unknown> & {
        updateComplete: Promise<unknown>
        checkValidity(): boolean
        validity: ValidityState
      }
    Object.assign(field, {
      type: 'number',
      inputMode: 'numeric',
      min: '1',
      max: '10',
      step: '1',
      enterKeyHint: 'done',
      autocapitalize: 'off',
      maxlength: 5,
      minlength: 1,
      pattern: '[0-9]+',
      value: '99'
    })
    document.body.append(field)
    await field.updateComplete
    const input = field.shadowRoot!.querySelector('input')!
    return {
      attrs: Object.fromEntries(
        [
          'type',
          'inputmode',
          'min',
          'max',
          'step',
          'enterkeyhint',
          'autocapitalize',
          'maxlength',
          'minlength',
          'pattern'
        ].map((a) => [a, input.getAttribute(a)])
      ),
      overflow: [field.checkValidity(), field.validity.rangeOverflow]
    }
  })
  expect(result.attrs).toEqual({
    type: 'number',
    inputmode: 'numeric',
    min: '1',
    max: '10',
    step: '1',
    enterkeyhint: 'done',
    autocapitalize: 'off',
    maxlength: '5',
    minlength: '1',
    pattern: '[0-9]+'
  })
  expect(result.overflow).toEqual([false, true])
})
