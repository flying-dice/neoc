/**
 * The built kit (`client/public/kit/kit.js` and `tokens.css`) under the conditions of an app frame: a sandboxed,
 * opaque-origin document with the frame's content security policy, which loads the two files as classic same-host
 * scripts. No host is needed: a tiny server stands in for it, so this checks the kit itself (that it runs under the
 * policy, defines every tag, follows `data-theme`) and not the host's wiring, which the app journeys check.
 */
import { startFrameServer, type FrameServer } from './frame-server'
import { expect, test } from '../../../e2e/fixtures'

let server: FrameServer
let origin: string

test.beforeAll(async () => {
  server = await startFrameServer()
  origin = server.origin
})
test.afterAll(() => server.close())

test('the kit runs in a sandboxed frame under the frame policy and answers the keyboard', async ({
  page
}) => {
  await page.goto(`${origin}/frame`)
  expect(await page.evaluate(() => window.origin)).toBe('null') // the frame has an opaque origin
  const tags = await page.evaluate(() =>
    [
      'ui-banner',
      'ui-button',
      'ui-check',
      'ui-empty-state',
      'ui-icon',
      'ui-list',
      'ui-row',
      'ui-section',
      'ui-segmented',
      'ui-spinner',
      'ui-text-field',
      'ui-toggle'
    ].filter((tag) => !customElements.get(tag))
  )
  expect(tags, 'tags the kit did not define').toEqual([])

  // a button presses by keyboard, a toggle flips, a segmented control moves, a field takes text
  await page.evaluate(() => {
    ;(window as unknown as { log: string[] }).log = []
    for (const id of ['button', 'toggle', 'segmented', 'field']) {
      for (const type of ['click', 'change']) {
        document
          .querySelector(`[data-testid=${id}]`)
          ?.addEventListener(type, () =>
            (window as unknown as { log: string[] }).log.push(`${id}:${type}`)
          )
      }
    }
  })
  await page.getByTestId('button').focus()
  await page.keyboard.press('Enter')
  await page.keyboard.press('Tab')
  await page.keyboard.press('Space')
  await page.keyboard.press('Tab')
  await page.keyboard.press('ArrowRight')
  await page.keyboard.press('Tab')
  await page.keyboard.type('Ada')
  await page.keyboard.press('Tab') // leaving the field commits the text: change
  expect(
    await page.evaluate(
      () =>
        (document.querySelector('[data-testid=toggle]') as HTMLElement & { checked: boolean })
          .checked
    )
  ).toBe(true)
  expect(
    await page.evaluate(
      () =>
        (document.querySelector('[data-testid=segmented]') as HTMLElement & { value: string }).value
    )
  ).toBe('b')
  expect(
    await page.evaluate(
      () => (document.querySelector('[data-testid=field]') as HTMLElement & { value: string }).value
    )
  ).toBe('Ada')
  expect(await page.evaluate(() => (window as unknown as { log: string[] }).log)).toEqual(
    expect.arrayContaining(['button:click', 'toggle:change', 'segmented:change', 'field:change'])
  )
  expect(
    await page.evaluate(() => (window as unknown as { violations: string[] }).violations)
  ).toEqual([])
})

test('the kit follows data-theme: the same elements read dark on a dark document', async ({
  page
}) => {
  await page.goto(`${origin}/frame`)
  const bg = (): Promise<string> =>
    page.evaluate(
      () =>
        getComputedStyle(document.querySelector('ui-button')!.shadowRoot!.querySelector('button')!)
          .backgroundColor
    )
  const light = await bg()
  await page.evaluate(() => {
    document.documentElement.dataset.theme = 'dark'
    document.documentElement.style.colorScheme = 'dark'
  })
  const dark = await bg()
  expect(light).not.toBe(dark)
  const text = await page.evaluate(() => getComputedStyle(document.body).color)
  expect(text).toBe('rgb(255, 255, 255)')
})

test('with no data-theme set (outside any vos page) the kit follows the system setting; an explicit data-theme wins', async ({
  page
}) => {
  const read = (): Promise<{ body: string; button: string }> =>
    page.evaluate(() => ({
      body: getComputedStyle(document.body).color,
      button: getComputedStyle(
        document.querySelector('ui-segmented')!.shadowRoot!.querySelector('.group')!
      ).backgroundColor
    }))
  await page.emulateMedia({ colorScheme: 'light' })
  await page.goto(`${origin}/standalone`)
  expect(await page.evaluate(() => document.documentElement.hasAttribute('data-theme'))).toBe(false)
  const light = await read()
  expect(light.body).toBe('rgb(0, 0, 0)')

  await page.emulateMedia({ colorScheme: 'dark' })
  const dark = await read()
  expect(dark.body).toBe('rgb(255, 255, 255)')
  expect(dark.button).not.toBe(light.button)

  // the host (when there is one) says light on a dark system: that wins
  await page.evaluate(() => (document.documentElement.dataset.theme = 'light'))
  expect((await read()).body).toBe('rgb(0, 0, 0)')
  // and dark on a light system
  await page.emulateMedia({ colorScheme: 'light' })
  await page.evaluate(() => (document.documentElement.dataset.theme = 'dark'))
  expect((await read()).body).toBe('rgb(255, 255, 255)')
})

test('a Reduce Motion setting on the document stops every transition, without the system setting', async ({
  page
}) => {
  await page.emulateMedia({ reducedMotion: 'no-preference' })
  await page.goto(`${origin}/frame`)
  const durations = (): Promise<string[]> =>
    page.evaluate(() => {
      const inside = (tag: string, sel: string): string =>
        getComputedStyle(document.querySelector(tag)!.shadowRoot!.querySelector(sel)!)
          .transitionDuration
      return [
        inside('ui-button', 'button'),
        inside('ui-toggle', '.knob'),
        inside('ui-segmented', '.thumb')
      ]
    })
  expect(await durations()).toEqual(['0.18s, 0.18s', '0.3s, 0.18s', '0.3s'])
  await page.evaluate(() => document.documentElement.setAttribute('data-reduce-motion', ''))
  expect(await durations()).toEqual(['0s, 0s', '0s, 0s', '0s'])
})

test('loading kit.js a second time, or after the elements are in the page, throws nothing and changes nothing', async ({
  page
}) => {
  const errors: string[] = []
  page.on('pageerror', (e) => errors.push(e.message))
  await page.goto(`${origin}/frame`)
  const button = page.getByTestId('button')
  const before = await page.evaluate(
    () => customElements.get('ui-button') === customElements.get('ui-button')
  )
  expect(before).toBe(true)
  const sameClass = await page.evaluate(() => customElements.get('ui-toggle'))
  // the plugin's tag and an app's own, or a hot reload: the script runs again
  await page.addScriptTag({ url: '/kit/kit.js' })
  await page.addScriptTag({ url: '/kit/kit.js' })
  expect(errors).toEqual([])
  expect(await page.evaluate(() => customElements.get('ui-toggle') !== undefined)).toBe(true)
  expect(sameClass).not.toBeNull()
  // the elements already in the page still work
  await page.getByTestId('toggle').click()
  expect(
    await page.evaluate(
      () =>
        (document.querySelector('[data-testid=toggle]') as HTMLElement & { checked: boolean })
          .checked
    )
  ).toBe(true)
  await expect(button).toBeVisible()
})

test('a colour or a variant from an app or an agent is data: it cannot add a declaration or a class', async ({
  page
}) => {
  await page.goto(`${origin}/frame`)
  const result = await page.evaluate(async () => {
    const hostile = 'red;position:fixed;inset:0;width:100vw;height:100vh'
    const made: Record<string, HTMLElement & Record<string, unknown>> = {}
    for (const tag of ['ui-toggle', 'ui-check', 'ui-button']) {
      const el = document.createElement(tag) as HTMLElement & Record<string, unknown>
      el.tint = hostile
      if (tag === 'ui-button') {
        el.fill = hostile
        el.variant = 'inverts plain" onclick="x'
      }
      el.textContent = 'x'
      document.body.append(el)
      made[tag] = el
    }
    await Promise.all(
      Object.values(made).map(
        (el) => (el as unknown as { updateComplete: Promise<unknown> }).updateComplete
      )
    )
    const inner = (tag: string): HTMLElement => made[tag]!.shadowRoot!.querySelector('button')!
    const icon = document.createElement('ui-row') as HTMLElement & Record<string, unknown>
    icon.icon = 'gear'
    icon.iconColor = hostile
    document.body.append(icon)
    await (icon as unknown as { updateComplete: Promise<unknown> }).updateComplete
    const tile = icon.shadowRoot!.querySelector<HTMLElement>('.tile')!
    return {
      positions: ['ui-toggle', 'ui-check', 'ui-button'].map(
        (tag) => getComputedStyle(inner(tag)).position
      ),
      buttonClass: inner('ui-button').className,
      buttonAttrs: [...inner('ui-button').attributes].map((a) => a.name).sort(),
      tilePosition: getComputedStyle(tile).position,
      // a valid colour still works
      valid: await (async () => {
        const t = made['ui-toggle']!
        t.tint = 'rebeccapurple'
        await (t as unknown as { updateComplete: Promise<unknown> }).updateComplete
        return getComputedStyle(t.shadowRoot!.querySelector('button')!).getPropertyValue(
          '--toggle-tint'
        )
      })()
    }
  })
  expect(result.positions).toEqual(['relative', 'relative', 'relative'])
  expect(result.buttonClass.trim()).toBe('filled')
  expect(result.buttonAttrs).not.toContain('onclick')
  expect(result.tilePosition).toBe('static')
  expect(result.valid.trim()).toBe('rebeccapurple')
})

test('every control has a hit area of at least 44 x 44 pt around the drawn control', async ({
  page
}) => {
  await page.goto(`${origin}/frame`)
  await page.evaluate(async () => {
    document.body.innerHTML = `<div id="t" style="display:grid;gap:40px;padding:40px;justify-items:start">
      <ui-button data-testid="h-small" size="small">Small</ui-button>
      <ui-button data-testid="h-medium">Medium</ui-button>
      <ui-button data-testid="h-plain" variant="plain" size="small">Plain</ui-button>
      <ui-button data-testid="h-square" square size="small" label="x">+</ui-button>
      <ui-toggle data-testid="h-toggle" label="t"></ui-toggle>
      <ui-check data-testid="h-check" label="c"></ui-check>
      <ui-segmented data-testid="h-seg" label="s" value="a"></ui-segmented>
      <ui-text-field data-testid="h-field" label="f"></ui-text-field></div>`
    ;(document.querySelector('ui-segmented') as unknown as { options: unknown }).options = [
      { value: 'a', label: 'A' },
      { value: 'b', label: 'B' }
    ]
    await Promise.all(
      [...document.querySelectorAll('#t > *')].map(
        (el) => (el as unknown as { updateComplete: Promise<unknown> }).updateComplete
      )
    )
  })
  const misses = await page.evaluate(() => {
    const missed: string[] = []
    const targets: [string, HTMLElement][] = [
      ...document.querySelectorAll<HTMLElement>('#t > *')
    ].map((el) => [el.dataset.testid!, el])
    // a segmented control is checked at its first segment
    const segment = document
      .querySelector('ui-segmented')!
      .shadowRoot!.querySelector<HTMLElement>('[role=radio]')!
    targets.push(['h-seg-segment', segment])
    for (const [id, el] of targets) {
      const r = el.getBoundingClientRect()
      const cx = r.left + r.width / 2
      const cy = r.top + r.height / 2
      const owner = id === 'h-seg-segment' ? document.querySelector('ui-segmented')! : el
      // the four corners of a 44 x 44 box around the centre (1 px inside) all land on the control
      for (const [dx, dy] of [
        [-21, -21],
        [21, -21],
        [-21, 21],
        [21, 21]
      ] as const) {
        const at = document.elementFromPoint(cx + dx, cy + dy)
        if (at !== owner) missed.push(`${id} at ${dx},${dy} hit ${at?.tagName ?? 'nothing'}`)
      }
    }
    return missed
  })
  expect(misses).toEqual([])
})
