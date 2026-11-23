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
      'ui-alert',
      'ui-banner',
      'ui-button',
      'ui-check',
      'ui-empty-state',
      'ui-icon',
      'ui-list',
      'ui-picker',
      'ui-row',
      'ui-section',
      'ui-segmented',
      'ui-spinner',
      'ui-split-view',
      'ui-text-area',
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

test('with hit, every control has a hit area of at least 44 x 44 pt around the drawn control', async ({
  page
}) => {
  // tall enough that every control is on screen: the points are asked of the viewport
  await page.setViewportSize({ width: 900, height: 1200 })
  await page.goto(`${origin}/frame`)
  await page.evaluate(async () => {
    const grid = document.createElement('div')
    grid.id = 't'
    grid.style.cssText = 'display:grid;gap:40px;padding:40px;justify-items:start'
    const make = (tag: string, id: string, attrs: Record<string, string>, text = ''): void => {
      const el = document.createElement(tag)
      el.dataset.testid = id
      for (const [name, value] of Object.entries(attrs)) el.setAttribute(name, value)
      el.textContent = text
      grid.append(el)
    }
    make('ui-button', 'h-small', { size: 'small', hit: '' }, 'Small')
    make('ui-button', 'h-medium', { hit: '' }, 'Medium')
    make('ui-button', 'h-plain', { variant: 'plain', size: 'small', hit: '' }, 'Plain')
    make('ui-button', 'h-square', { square: '', size: 'small', label: 'x', hit: '' }, '+')
    make('ui-toggle', 'h-toggle', { label: 't', hit: '' })
    make('ui-check', 'h-check', { label: 'c', hit: '' })
    make('ui-segmented', 'h-seg', { label: 's', value: 'a', hit: '' })
    make('ui-text-field', 'h-field', { label: 'f', hit: '' })
    make('ui-text-area', 'h-area', { label: 'a', rows: '1', hit: '' })
    make('ui-picker', 'h-picker', { label: 'p', hit: '' })
    document.body.replaceChildren(grid)
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
      // a segment grows vertically only: segments sit side by side
      const corners =
        id === 'h-seg-segment'
          ? ([
              [0, -21],
              [0, 21]
            ] as const)
          : ([
              [-21, -21],
              [21, -21],
              [-21, 21],
              [21, 21]
            ] as const)
      for (const [dx, dy] of corners) {
        const at = document.elementFromPoint(cx + dx, cy + dy)
        if (at !== owner) missed.push(`${id} at ${dx},${dy} hit ${at?.tagName ?? 'nothing'}`)
      }
    }
    return missed
  })
  expect(misses).toEqual([])
})

for (const hit of [false, true]) {
  test(`a tap inside a control's drawn box always activates that control, never a neighbour (hit ${hit ? 'on' : 'off'})`, async ({
    page
  }) => {
    await page.goto(`${origin}/frame`)
    // A control's enlarged hit area is layout space it owns, so it can never cover another control's drawn box, with
    // hit on or off. Layouts where controls sit close: one-letter segments (no gap), two checks 8 px apart, a toggle and
    // a check 6 px apart, two small buttons 4 px apart, and rows: a leading check with a trailing toggle, two trailing
    // checks, a trailing icon button then a check, and two leading checks (a row gives its controls the 44 pt box).
    const points = await page.evaluate(async (hit) => {
      const w = window as unknown as { log: string[] }
      w.log = []
      const log = (id: string) => () => w.log.push(id)
      const area = document.createElement('div')
      area.style.cssText = 'display:grid;gap:40px;padding:40px;justify-items:start;width:420px'
      const strip = (gap: number): HTMLDivElement => {
        const d = document.createElement('div')
        d.style.cssText = `display:flex;align-items:center;gap:${gap}px`
        area.append(d)
        return d
      }
      const make = (
        tag: string,
        id: string,
        attrs: Record<string, string> = {},
        text = ''
      ): HTMLElement => {
        const el = document.createElement(tag)
        el.dataset.testid = id
        for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
        if (hit && tag !== 'ui-row' && tag !== 'ui-list' && tag !== 'ui-section')
          el.setAttribute('hit', '')
        el.textContent = text
        return el
      }
      const seg = make('ui-segmented', 'seg', { label: 's' }) as HTMLElement & { options: unknown }
      seg.options = ['A', 'B', 'C'].map((v) => ({ value: v, label: v }))
      area.append(seg)
      strip(8).append(
        make('ui-check', 'check-1', { label: '1' }),
        make('ui-check', 'check-2', { label: '2' })
      )
      strip(6).append(
        make('ui-toggle', 'toggle', { label: 't' }),
        make('ui-check', 'check-3', { label: '3' })
      )
      strip(4).append(
        make('ui-button', 'button-1', { size: 'small', variant: 'tinted' }, 'A'),
        make('ui-button', 'button-2', { size: 'small', variant: 'tinted' }, 'B')
      )
      // rows: [row id, tappable, [slot, control]...]
      const rows: [string, boolean, [string, HTMLElement][]][] = [
        [
          'row',
          true,
          [
            ['leading', make('ui-check', 'row-check', { label: 'lead' })],
            ['trailing', make('ui-toggle', 'row-toggle', { label: 'trail' })]
          ]
        ],
        [
          'row-checks',
          true,
          [
            ['trailing', make('ui-check', 'trail-1', { label: 'a' })],
            ['trailing', make('ui-check', 'trail-2', { label: 'b' })]
          ]
        ],
        [
          'row-icon',
          true,
          [
            [
              'trailing',
              make(
                'ui-button',
                'icon-1',
                { square: '', size: 'small', variant: 'plain', label: 'i' },
                '+'
              )
            ],
            ['trailing', make('ui-check', 'trail-3', { label: 'c' })]
          ]
        ],
        [
          'row-leads',
          true,
          [
            ['leading', make('ui-check', 'lead-1', { label: 'a' })],
            ['leading', make('ui-check', 'lead-2', { label: 'b' })]
          ]
        ]
      ]
      const list = make('ui-list', 'list', { label: 'l' })
      list.style.cssText = 'width:380px'
      const section = make('ui-section', 'section')
      for (const [id, tappable, controls] of rows) {
        const row = make('ui-row', id, { heading: id, ...(tappable ? { tappable: '' } : {}) })
        for (const [slot, control] of controls) {
          control.setAttribute('slot', slot)
          row.append(control)
        }
        section.append(row)
      }
      list.append(section)
      area.append(list)
      document.body.replaceChildren(area)
      for (const el of area.querySelectorAll<HTMLElement>('[data-testid]')) {
        const id = el.dataset.testid!
        if (id.startsWith('row') && el.localName === 'ui-row')
          el.addEventListener('press', log(`${id}-press`))
        else if (id === 'seg')
          el.addEventListener('change', (e) => w.log.push(`seg:${(e as CustomEvent).detail.value}`))
        else if (id.startsWith('button') || id.startsWith('icon'))
          el.addEventListener('click', log(id))
        else if (id !== 'list' && id !== 'section') el.addEventListener('change', log(id))
      }
      await Promise.all(
        [...area.querySelectorAll('*')]
          .filter((e) => 'updateComplete' in e)
          .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
      )
      const out: { id: string; expect: string; x: number; y: number }[] = []
      const edges = (id: string, expectLog: string, r: DOMRect): void => {
        const y = r.top + r.height / 2
        out.push({ id: `${id} left`, expect: expectLog, x: r.left + 1.5, y })
        out.push({ id: `${id} right`, expect: expectLog, x: r.right - 1.5, y })
      }
      // the drawn box of each control (the inner button), by id; a segment is `seg-A` ...
      const drawn = (id: string): DOMRect =>
        area
          .querySelector(`[data-testid=${id}]`)!
          .shadowRoot!.querySelector('button')!
          .getBoundingClientRect()
      for (const v of ['A', 'B', 'C']) {
        const button = seg.shadowRoot!.querySelector(`[data-testid="seg-${v}"]`)!
        edges(`seg-${v}`, `seg:${v}`, button.getBoundingClientRect())
      }
      for (const id of [
        'check-1',
        'check-2',
        'check-3',
        'toggle',
        'button-1',
        'button-2',
        'row-check',
        'row-toggle',
        'trail-1',
        'trail-2',
        'icon-1',
        'trail-3',
        'lead-1',
        'lead-2'
      ])
        edges(id, id, drawn(id))
      // the row's own text area presses the row
      const text = area
        .querySelector('[data-testid=row]')!
        .shadowRoot!.querySelector('.text')!
        .getBoundingClientRect()
      out.push({
        id: 'row text',
        expect: 'row-press',
        x: text.left + 4,
        y: text.top + text.height / 2
      })
      return out
    }, hit)
    const wrong: string[] = []
    for (const p of points) {
      await page.evaluate(() => {
        ;(window as unknown as { log: string[] }).log = []
        const seg = document.querySelector('[data-testid=seg]') as HTMLElement & { value: string }
        seg.value = ''
      })
      await page.mouse.click(p.x, p.y)
      const log = await page.evaluate(() => (window as unknown as { log: string[] }).log)
      if (log.length !== 1 || log[0] !== p.expect)
        wrong.push(`${p.id}: expected ${p.expect}, got ${JSON.stringify(log)}`)
    }
    expect(wrong).toEqual([])
  })
}

for (const hit of [false, true]) {
  test(`the element is the control: a click in an empty part of a forced-size element does what a click on the drawn part does (hit ${hit ? 'on' : 'off'})`, async ({
    page
  }) => {
    // tall enough that every control is on screen: the clicks are at page coordinates
    await page.setViewportSize({ width: 900, height: 3200 })
    await page.goto(`${origin}/standalone`)
    // An app that forces a size onto a control (or a layout that stretches one: a button fills its cell, and with hit the
    // drawn button is centred in a stretched element) makes a larger element. The whole element is the control, as it
    // is for a native control with a forced size: a click anywhere on it does everything a click on the drawn part does,
    // not onclick without the change, or the change without the click, or a press without its submit.
    const plan = await page.evaluate(async (hit) => {
      const w = window as unknown as { log: string[] }
      w.log = []
      const kinds: [string, Record<string, string>, string][] = [
        ['ui-button', { variant: 'tinted', size: 'small', type: 'submit' }, 'Go'],
        ['ui-toggle', { label: 't' }, ''],
        ['ui-check', { label: 'c' }, ''],
        ['ui-segmented', { label: 's' }, ''],
        ['ui-text-field', { label: 'f' }, ''],
        ['ui-text-area', { label: 'a' }, ''],
        ['ui-picker', { label: 'p', value: 'A' }, '']
      ]
      const layouts: [string, string][] = [
        ['column', 'display:flex;flex-direction:column;align-items:stretch;width:300px'],
        ['grid', 'display:grid;grid-template-columns:300px;grid-auto-rows:110px'],
        ['forced', 'display:block']
      ]
      const form = document.createElement('form')
      form.addEventListener('submit', (e) => {
        e.preventDefault()
        w.log.push('submit')
      })
      form.style.cssText = 'display:grid;gap:24px;padding:24px'
      const items: { id: string; el: HTMLElement; forced: boolean }[] = []
      for (const [layout, css] of layouts)
        for (const [tag, attrs, text] of kinds) {
          const box = document.createElement('div')
          box.style.cssText = css
          const el = document.createElement(tag)
          for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
          if (hit) el.setAttribute('hit', '')
          el.textContent = text
          // an app that forces a size onto the control
          if (layout === 'forced') el.style.cssText = 'display:inline-flex;width:260px;height:100px'
          const id = `${layout}-${tag}`
          el.dataset.testid = id
          if (tag === 'ui-segmented' || tag === 'ui-picker')
            (el as HTMLElement & { options: unknown }).options = ['A', 'B'].map((v) => ({
              value: v,
              label: v
            }))
          box.append(el)
          form.append(box)
          for (const type of ['click', 'change', 'focusin'])
            el.addEventListener(type, () => w.log.push(`${type}`))
          items.push({ id, el, forced: layout === 'forced' })
        }
      document.body.replaceChildren(form)
      await Promise.all(
        items.map((i) => (i.el as unknown as { updateComplete: Promise<unknown> }).updateComplete)
      )
      // the controls that fill the width they are given
      const FIELDS = ['ui-text-field', 'ui-text-area', 'ui-picker']
      const out: { id: string; drawn: [number, number]; empty: [number, number][] }[] = []
      for (const { id, el } of items) {
        const host = el.getBoundingClientRect()
        const inner = el
          .shadowRoot!.querySelector<HTMLElement>(
            el.localName === 'ui-segmented'
              ? '.group'
              : el.localName === 'ui-text-field'
                ? 'input'
                : el.localName === 'ui-text-area'
                  ? 'textarea'
                  : el.localName === 'ui-picker'
                    ? 'select'
                    : 'button'
          )!
          .getBoundingClientRect()
        // the drawn part: a segmented control's first segment, the control itself otherwise
        const first =
          el.localName === 'ui-segmented'
            ? el.shadowRoot!.querySelector('.segment')!.getBoundingClientRect()
            : inner
        const cx = inner.left + inner.width / 2
        const cy = inner.top + inner.height / 2
        const halfW =
          hit && !FIELDS.includes(el.localName) ? Math.max(inner.width, 44) / 2 : inner.width / 2
        const halfH = hit ? Math.max(inner.height, 44) / 2 : inner.height / 2
        const candidates: [number, number][] = [
          [cx + halfW + 8, cy],
          [cx, cy + halfH + 6]
        ]
        const empty = candidates.filter(
          ([x, y]) => x > host.left && x < host.right && y > host.top && y < host.bottom
        )
        out.push({ id, drawn: [first.left + first.width / 2, first.top + first.height / 2], empty })
      }
      return out
    }, hit)
    const reset = (): Promise<void> =>
      page.evaluate(() => {
        ;(window as unknown as { log: string[] }).log = []
        for (const el of document.querySelectorAll<
          HTMLElement & { checked?: boolean; value?: string }
        >('ui-toggle, ui-check'))
          el.checked = false
        for (const el of document.querySelectorAll<HTMLElement & { value?: string }>(
          'ui-segmented'
        ))
          el.value = ''
        ;(document.activeElement as HTMLElement | null)?.blur()
      })
    const heard = async (x: number, y: number): Promise<string[]> => {
      await reset()
      await page.mouse.click(x, y)
      return page.evaluate(() => [...(window as unknown as { log: string[] }).log].sort())
    }
    let compared = 0
    const wrong: string[] = []
    for (const { id, drawn, empty } of plan) {
      const onDrawn = await heard(drawn[0], drawn[1])
      if (!onDrawn.length) wrong.push(`${id}: the drawn part heard nothing`)
      for (const [x, y] of empty) {
        const onEmpty = await heard(x, y)
        compared++
        // a segmented control chooses a segment: outside the segments it chooses nothing (no change)
        const expected = id.endsWith('ui-segmented')
          ? onEmpty.filter((e) => e !== 'change')
          : onDrawn
        if (
          JSON.stringify(onEmpty) !== JSON.stringify(expected) ||
          (id.endsWith('ui-segmented') && onEmpty.includes('change'))
        )
          wrong.push(
            `${id} at ${Math.round(x)},${Math.round(y)}: drawn ${JSON.stringify(onDrawn)}, empty ${JSON.stringify(onEmpty)}`
          )
      }
    }
    expect(compared, 'the layouts leave an empty part to click').toBeGreaterThan(6)
    expect(wrong).toEqual([])
  })
}

for (const hit of [false, true]) {
  test(`locator.click() on the element works in a normal and a stretched layout (hit ${hit ? 'on' : 'off'})`, async ({
    page
  }) => {
    await page.goto(`${origin}/frame`)
    // What a developer's Playwright test writes: click the element by its test id. The centre of the element is the drawn
    // control (a toggle, a check, a segmented control are fixed-size, the field does not stretch taller, a button fills
    // its cell), so it works in a flex column, a grid cell, a tall flex row and inline.
    const ids = await page.evaluate(async (hit) => {
      const w = window as unknown as { log: string[] }
      w.log = []
      const kinds: [string, string, Record<string, string>, string][] = [
        ['ui-button', 'click', { variant: 'tinted', size: 'small' }, 'Go'],
        ['ui-toggle', 'change', { label: 't' }, ''],
        ['ui-check', 'change', { label: 'c' }, ''],
        ['ui-segmented', 'change', { label: 's' }, ''],
        ['ui-text-field', 'focusin', { label: 'f' }, ''],
        ['ui-text-area', 'focusin', { label: 'a' }, ''],
        ['ui-picker', 'focusin', { label: 'p', value: 'A' }, '']
      ]
      const layouts: [string, string][] = [
        ['inline', 'display:block'],
        ['column', 'display:flex;flex-direction:column;align-items:stretch;width:300px'],
        ['grid', 'display:grid;grid-template-columns:300px;grid-auto-rows:110px'],
        ['row', 'display:flex;align-items:stretch;gap:12px']
      ]
      const area = document.createElement('div')
      area.style.cssText = 'display:grid;gap:24px;padding:24px'
      const made: { id: string; event: string; el: HTMLElement }[] = []
      for (const [layout, css] of layouts)
        for (const [tag, event, attrs, text] of kinds) {
          const box = document.createElement('div')
          box.style.cssText = css
          const el = document.createElement(tag)
          for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
          if (hit) el.setAttribute('hit', '')
          el.textContent = text
          const id = `${layout}-${tag}`
          el.dataset.testid = id
          if (tag === 'ui-segmented' || tag === 'ui-picker')
            (el as HTMLElement & { options: unknown }).options = ['A', 'B'].map((v) => ({
              value: v,
              label: v
            }))
          box.append(el)
          if (layout === 'row') {
            const tall = document.createElement('div')
            tall.style.cssText = 'height:120px;width:40px;background:#ddd'
            box.append(tall)
          }
          area.append(box)
          el.addEventListener(event, () => w.log.push(`${id}:${event}`))
          made.push({ id, event, el })
        }
      document.body.replaceChildren(area)
      await Promise.all(
        made.map((m) => (m.el as unknown as { updateComplete: Promise<unknown> }).updateComplete)
      )
      return made.map((m) => ({ id: m.id, event: m.event }))
    }, hit)
    const wrong: string[] = []
    for (const { id, event } of ids) {
      await page.evaluate(() => {
        ;(window as unknown as { log: string[] }).log = []
        ;(document.activeElement as HTMLElement | null)?.blur()
      })
      await page.getByTestId(id).click({ timeout: 3000 })
      const log = await page.evaluate(() => (window as unknown as { log: string[] }).log)
      if (log.length !== 1 || log[0] !== `${id}:${event}`)
        wrong.push(`${id}: ${JSON.stringify(log)}`)
    }
    expect(wrong).toEqual([])
  })
}

test('a click on the element itself, from a script, a test or a label, acts as a click on the control, once; disabled, it does nothing', async ({
  page
}) => {
  await page.goto(`${origin}/standalone`)
  const result = await page.evaluate(async () => {
    const w = window as unknown as { log: string[] }
    w.log = []
    const mk = (tag: string, attrs: Record<string, string> = {}, text = ''): HTMLElement => {
      const el = document.createElement(tag)
      for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
      el.textContent = text
      return el
    }
    const form = document.createElement('form')
    form.addEventListener('submit', (e) => {
      e.preventDefault()
      w.log.push('submit')
    })
    const field = mk('ui-text-field', {
      name: 'who',
      label: 'who',
      value: 'Ada',
      id: 'who'
    }) as HTMLElement & { value: string }
    const submit = mk('ui-button', { type: 'submit', id: 'submit' }, 'Go')
    const reset = mk('ui-button', { type: 'reset', id: 'reset' }, 'Reset')
    const off = mk('ui-button', { type: 'submit', disabled: '', id: 'off' }, 'Off')
    const toggle = mk('ui-toggle', { label: 't', id: 'toggle' })
    const check = mk('ui-check', { label: 'c', id: 'check' })
    const offToggle = mk('ui-toggle', { label: 'o', disabled: '', id: 'off-toggle' })
    const label = mk('label', { for: 'submit' }, 'Go label')
    const fieldLabel = mk('label', { for: 'who' }, 'Who label')
    form.append(field, fieldLabel, submit, reset, off, toggle, check, offToggle, label)
    const list = mk('ui-list', { label: 'l' })
    const section = mk('ui-section')
    const row = mk('ui-row', { heading: 'Row', tappable: '', id: 'row' })
    const offRow = mk('ui-row', { heading: 'Off', tappable: '', disabled: '', id: 'off-row' })
    section.append(row, offRow)
    list.append(section)
    document.body.replaceChildren(form, list)
    for (const el of [submit, reset, off, toggle, check, offToggle, row, offRow]) {
      el.addEventListener('click', () => w.log.push(`${el.id}:click`))
      el.addEventListener('change', () => w.log.push(`${el.id}:change`))
      el.addEventListener('press', () => w.log.push(`${el.id}:press`))
    }
    await Promise.all(
      [...document.querySelectorAll('*')]
        .filter((e) => 'updateComplete' in e)
        .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
    )
    const out: Record<string, unknown> = {}
    const run = async (name: string, act: () => void): Promise<void> => {
      w.log = []
      act()
      await new Promise((r) => setTimeout(r, 30))
      // the control acts first and the app's own listener on the element hears the click after it: order aside, each once
      out[name] = [...w.log].sort()
    }
    await run('submit.click()', () => submit.click())
    await run('label click', () => label.click())
    await run('real click on the drawn button', () =>
      submit.shadowRoot!.querySelector('button')!.click()
    )
    field.value = 'Grace'
    await (field as unknown as { updateComplete: Promise<unknown> }).updateComplete
    await run('reset.click()', () => reset.click())
    out.afterReset = field.value
    ;(document.activeElement as HTMLElement | null)?.blur()
    fieldLabel.click()
    out.labelFocusesField = document.activeElement?.id
    await run('disabled button.click()', () => off.click())
    await run('toggle.click()', () => toggle.click())
    await run('check.click()', () => check.click())
    await run('disabled toggle.click()', () => offToggle.click())
    await run('row.click()', () => row.click())
    await run('disabled row.click()', () => offRow.click())
    await run('real tap on the row button', () =>
      row.shadowRoot!.querySelector<HTMLElement>('.hit')!.click()
    )
    return out
  })
  expect(result).toEqual({
    'submit.click()': ['submit', 'submit:click'],
    'label click': ['submit', 'submit:click'],
    'real click on the drawn button': ['submit', 'submit:click'],
    'reset.click()': ['reset:click'],
    afterReset: 'Ada',
    labelFocusesField: 'who',
    'disabled button.click()': [],
    'toggle.click()': ['toggle:change', 'toggle:click'],
    'check.click()': ['check:change', 'check:click'],
    'disabled toggle.click()': [],
    'row.click()': ['row:click', 'row:press'],
    'disabled row.click()': [],
    'real tap on the row button': ['row:click', 'row:press']
  })
})

test('Enter and Space press a button, flip a toggle and a check, and press a tappable row, once each', async ({
  page
}) => {
  await page.goto(`${origin}/standalone`)
  await page.evaluate(async () => {
    const w = window as unknown as { log: string[] }
    w.log = []
    const mk = (tag: string, attrs: Record<string, string> = {}, text = ''): HTMLElement => {
      const el = document.createElement(tag)
      for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
      el.textContent = text
      return el
    }
    const list = mk('ui-list', { label: 'l' })
    const section = mk('ui-section')
    const row = mk('ui-row', { heading: 'Row', tappable: '', id: 'row' })
    section.append(row)
    list.append(section)
    document.body.replaceChildren(
      mk('ui-button', { id: 'button' }, 'Go'),
      mk('ui-toggle', { id: 'toggle', label: 't' }),
      mk('ui-check', { id: 'check', label: 'c' }),
      list
    )
    for (const el of document.querySelectorAll<HTMLElement>('#button, #toggle, #check, #row')) {
      el.addEventListener('click', () => w.log.push(`${el.id}:click`))
      el.addEventListener('change', () => w.log.push(`${el.id}:change`))
      el.addEventListener('press', () => w.log.push(`${el.id}:press`))
    }
    await Promise.all(
      [...document.querySelectorAll('*')]
        .filter((e) => 'updateComplete' in e)
        .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
    )
  })
  const heard = async (id: string, key: string): Promise<string[]> => {
    await page.evaluate(() => ((window as unknown as { log: string[] }).log = []))
    // a tappable row's focus target is the button inside it
    await page.evaluate((id) => {
      const el = document.getElementById(id) as HTMLElement & { focusRow?: () => void }
      if (el.focusRow) el.focusRow()
      else el.focus()
    }, id)
    await page.keyboard.press(key)
    return page.evaluate(() => [...(window as unknown as { log: string[] }).log].sort())
  }
  expect(await heard('button', 'Enter')).toEqual(['button:click'])
  expect(await heard('button', 'Space')).toEqual(['button:click'])
  expect(await heard('toggle', 'Space')).toEqual(['toggle:change', 'toggle:click'])
  expect(await heard('check', 'Space')).toEqual(['check:change', 'check:click'])
  expect(await heard('row', 'Enter')).toEqual(['row:click', 'row:press'])
  expect(await heard('row', 'Space')).toEqual(['row:click', 'row:press'])
})

// "Disabled" means one thing for every control, in every place: a disabled control, however it is disabled, hears nothing,
// activates nothing, submits nothing, cannot be focused, swallows the click so that the parent hears nothing either, and
// looks disabled. The native disabled button is the control experiment for the click cases.
for (const way of [
  'its own attribute',
  'its property, set after it was drawn',
  'a disabled fieldset'
] as const) {
  test(`a control disabled by ${way} does nothing and lets nothing through, however it is activated`, async ({
    page
  }) => {
    await page.setViewportSize({ width: 900, height: 2600 })
    await page.goto(`${origin}/standalone`)
    const cases = await page.evaluate(async (way) => {
      const w = window as unknown as { log: string[] }
      w.log = []
      const kinds = [
        'native',
        'ui-button',
        'ui-toggle',
        'ui-check',
        'ui-segmented',
        'ui-text-field',
        'ui-text-area',
        'ui-picker'
      ] as const
      const mk = (tag: string, attrs: Record<string, string> = {}, text = ''): HTMLElement => {
        const el = document.createElement(tag)
        for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
        el.textContent = text
        return el
      }
      const area = document.createElement('div')
      area.style.cssText = 'display:grid;gap:16px;padding:16px;width:600px'
      const out: { id: string; kind: string; context: string; hasLabel: boolean }[] = []
      const controls: (HTMLElement & { disabled: boolean })[] = []
      const listen = (el: HTMLElement, id: string): void => {
        for (const type of ['click', 'change', 'focusin', 'press'])
          el.addEventListener(type, () => w.log.push(`${id}:${type}`))
      }
      // context 1: a control inside a clickable card inside a form; context 2: a trailing control of a tappable row
      for (const context of ['card', 'row'] as const) {
        for (const kind of kinds) {
          const id = `${context}-${kind}`
          const form = document.createElement('form')
          form.addEventListener('submit', (e) => {
            e.preventDefault()
            w.log.push(`${id}:submit`)
          })
          const fieldset = document.createElement('fieldset')
          fieldset.style.cssText = 'border:0;margin:0;padding:0;min-width:0'
          if (way === 'a disabled fieldset') fieldset.disabled = true
          const off = way === 'its own attribute' ? { disabled: '' } : {}
          let control: HTMLElement
          if (kind === 'native') control = mk('button', { type: 'submit', ...off }, 'Go')
          else if (kind === 'ui-button') control = mk('ui-button', { type: 'submit', ...off }, 'Go')
          else if (kind === 'ui-segmented' || kind === 'ui-picker') {
            control = mk(kind, { label: 's', ...off })
            ;(control as HTMLElement & { options: unknown }).options = ['A', 'B'].map((v) => ({
              value: v,
              label: v
            }))
          } else control = mk(kind, { label: kind, ...off })
          control.dataset.testid = id
          control.id = id
          controls.push(control as HTMLElement & { disabled: boolean })
          listen(control, id)
          const label = mk('label', { for: id }, 'label')
          label.addEventListener('click', () => undefined)
          const card = mk('div')
          card.style.cssText = 'padding:12px;background:#eee'
          card.dataset.testid = `${id}-card`
          card.addEventListener('click', () => w.log.push(`${id}:card`))
          if (context === 'card') {
            card.append(control)
          } else {
            const list = mk('ui-list', { label: 'l' })
            const section = mk('ui-section')
            const row = mk('ui-row', { heading: 'Row', tappable: '', id: `${id}-row` })
            row.addEventListener('press', () => w.log.push(`${id}:row-press`))
            control.setAttribute('slot', 'trailing')
            row.append(control)
            section.append(row)
            list.append(section)
            card.append(list)
          }
          // the label is outside the card: a click on a label is the label's own, and the card would hear it
          fieldset.append(card, label)
          form.append(fieldset)
          area.append(form)
          out.push({ id, kind, context, hasLabel: kind !== 'ui-segmented' })
        }
      }
      // a disabled tappable row and a disabled option row, in a clickable card
      for (const kind of ['row', 'option-row'] as const) {
        const id = `row-${kind}`
        const card = mk('div')
        card.style.cssText = 'padding:12px;background:#eee'
        card.addEventListener('click', () => w.log.push(`${id}:card`))
        const list = mk(
          'ui-list',
          kind === 'option-row' ? { label: 'l', selection: 'single' } : { label: 'l' }
        )
        const section = mk('ui-section')
        const row = mk('ui-row', {
          heading: 'Row',
          tappable: '',
          id,
          'data-testid': id,
          ...(way === 'its own attribute' ? { disabled: '' } : { disabled: '' })
        })
        row.addEventListener('press', () => w.log.push(`${id}:press`))
        row.addEventListener('click', () => w.log.push(`${id}:click`))
        section.append(row)
        list.append(section)
        card.append(list)
        area.append(card)
        out.push({ id, kind, context: 'card', hasLabel: false })
      }
      document.body.replaceChildren(area)
      const settled = (): Promise<unknown> =>
        Promise.all(
          [...document.querySelectorAll('*')]
            .filter((e) => 'updateComplete' in e)
            .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
        )
      await settled()
      // the everyday `disabled={busy}`: drawn enabled, then disabled by its property
      if (way === 'its property, set after it was drawn') {
        for (const c of controls) c.disabled = true
        await settled()
        await settled()
      }
      return out
    }, way)
    const centre = (id: string): Promise<{ x: number; y: number }> =>
      page.evaluate((id) => {
        const el = document.getElementById(id)!
        const inner =
          el.shadowRoot?.querySelector('button, input, textarea, select, .group, .face') ?? el
        const r = inner.getBoundingClientRect()
        return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
      }, id)
    const reset = (): Promise<void> =>
      page.evaluate(() => {
        ;(window as unknown as { log: string[] }).log = []
        ;(document.activeElement as HTMLElement | null)?.blur()
      })
    const heard = (): Promise<string[]> =>
      page.evaluate(() => [...(window as unknown as { log: string[] }).log])
    const wrong: string[] = []
    const ways: [string, (id: string, label: boolean) => Promise<void>][] = [
      [
        'a real click',
        async (id) => {
          const { x, y } = await centre(id)
          await page.mouse.click(x, y)
        }
      ],
      [
        'locator.click()',
        async (id) => {
          await page.locator(`#${id}`).click({ force: true, timeout: 2000 })
        }
      ],
      [
        'el.click()',
        async (id) => {
          await page.evaluate((id) => document.getElementById(id)!.click(), id)
        }
      ],
      [
        'a label click',
        async (id, label) => {
          if (label)
            await page.evaluate(
              (id) => (document.querySelector(`label[for="${id}"]`) as HTMLElement).click(),
              id
            )
        }
      ],
      [
        'Enter and Space',
        async (id) => {
          await page.evaluate((id) => {
            const el = document.getElementById(id) as HTMLElement & { focusRow?: () => void }
            if (el.focusRow) el.focusRow()
            else el.focus()
          }, id)
          await page.keyboard.press('Enter')
          await page.keyboard.press('Space')
        }
      ]
    ]
    for (const c of cases) {
      for (const [name, act] of ways) {
        await reset()
        await act(c.id, c.hasLabel)
        const log = await heard()
        if (log.length)
          wrong.push(`${c.id} (${c.kind}, ${c.context}) by ${way}, ${name}: ${JSON.stringify(log)}`)
      }
      // it cannot be focused, whatever the call
      await reset()
      const focused = await page.evaluate((id) => {
        const el = document.getElementById(id) as HTMLElement & { focusRow?: () => void }
        if (el.focusRow) el.focusRow()
        else el.focus()
        return (
          document.activeElement === el ||
          (document.activeElement !== document.body && !!el.contains(document.activeElement))
        )
      }, c.id)
      if (focused) wrong.push(`${c.id} (${c.kind}) took focus`)
      // and it looks disabled
      const dim = await page.evaluate((id) => {
        const el = document.getElementById(id)!
        const inner =
          el.shadowRoot?.querySelector<HTMLElement>('button, input, textarea, select, .segment') ??
          el
        const target =
          el.localName === 'ui-row' ? el.shadowRoot!.querySelector<HTMLElement>('.content')! : inner
        return Number(getComputedStyle(target).opacity)
      }, c.id)
      // (the native button is the control experiment for the clicks; its look is the browser's)
      if (c.kind !== 'native' && !(dim < 1))
        wrong.push(`${c.id} (${c.kind}) does not look disabled (opacity ${dim})`)
    }
    expect(wrong).toEqual([])
  })
}

// A control that is disabled and then enabled again, however each was done, hears, activates and takes focus again, and
// one that is disabled in two ways stays disabled until both are undone. The everyday case is `disabled={busy}`:
// the property, set after the control was drawn, then cleared. Every control (and the native button as the experiment)
// in every route is one cell of this table.
const KINDS = [
  'native',
  'ui-button',
  'ui-toggle',
  'ui-check',
  'ui-segmented',
  'ui-text-field',
  'ui-text-area',
  'ui-picker'
] as const
/** The controls whose answer to a click is to take the focus (a field to type in, a picker whose menu is the system's). */
const TAKES_FOCUS: readonly string[] = ['ui-text-field', 'ui-text-area', 'ui-picker']
type Step =
  'property on' | 'property off' | 'attribute on' | 'attribute off' | 'fieldset on' | 'fieldset off'
const ROUTES: { name: string; steps: Step[]; deadAfter: boolean[] }[] = [
  // each step, and whether the control is still dead after it
  { name: 'its property', steps: ['property on', 'property off'], deadAfter: [true, false] },
  { name: 'its attribute', steps: ['attribute on', 'attribute off'], deadAfter: [true, false] },
  { name: 'a fieldset', steps: ['fieldset on', 'fieldset off'], deadAfter: [true, false] },
  {
    name: 'the property and a fieldset, the property undone first',
    steps: ['property on', 'fieldset on', 'property off', 'fieldset off'],
    deadAfter: [true, true, true, false]
  },
  {
    name: 'the fieldset and the property, the fieldset undone first',
    steps: ['fieldset on', 'property on', 'fieldset off', 'property off'],
    deadAfter: [true, true, true, false]
  },
  {
    name: 'the property and the attribute, the attribute undone first',
    steps: ['property on', 'attribute on', 'attribute off', 'property off'],
    deadAfter: [true, true, false, false]
  }
]

test('a control disabled and enabled again, by its property, its attribute or a fieldset, hears, activates and focuses again', async ({
  page
}) => {
  await page.setViewportSize({ width: 900, height: 900 })
  await page.goto(`${origin}/standalone`)
  const wrong: string[] = []
  for (const route of ROUTES) {
    for (const kind of KINDS) {
      const id = 'cell'
      await page.evaluate(
        async ({ kind, id }) => {
          const w = window as unknown as { log: string[] }
          w.log = []
          const form = document.createElement('form')
          form.addEventListener('submit', (e) => {
            e.preventDefault()
            w.log.push('submit')
          })
          const fieldset = document.createElement('fieldset')
          fieldset.style.cssText = 'border:0;margin:0;padding:12px;min-width:0'
          let control: HTMLElement
          if (kind === 'native' || kind === 'ui-button') {
            control = document.createElement(kind === 'native' ? 'button' : 'ui-button')
            control.setAttribute('type', 'submit')
            control.textContent = 'Go'
          } else {
            control = document.createElement(kind)
            control.setAttribute('label', kind)
            if (kind === 'ui-segmented' || kind === 'ui-picker')
              (control as HTMLElement & { options: unknown }).options = ['A', 'B'].map((v) => ({
                value: v,
                label: v
              }))
          }
          control.id = id
          for (const type of ['click', 'change', 'focusin'])
            control.addEventListener(type, () => w.log.push(type))
          fieldset.append(control)
          form.append(fieldset)
          document.body.replaceChildren(form)
          await Promise.all(
            [...document.querySelectorAll('*')]
              .filter((e) => 'updateComplete' in e)
              .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
          )
        },
        { kind, id }
      )
      const drive = (step: Step): Promise<void> =>
        page.evaluate(async (step) => {
          const el = document.getElementById('cell') as HTMLElement & { disabled: boolean }
          const fieldset = el.closest('fieldset')!
          if (step === 'property on') el.disabled = true
          else if (step === 'property off') el.disabled = false
          else if (step === 'attribute on') el.setAttribute('disabled', '')
          else if (step === 'attribute off') el.removeAttribute('disabled')
          else fieldset.disabled = step === 'fieldset on'
          // twice: a framework sets a property, and the control draws on the next microtask, and again for what that did
          for (let i = 0; i < 2; i++)
            await Promise.all(
              [...document.querySelectorAll('*')]
                .filter((e) => 'updateComplete' in e)
                .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
            )
        }, step)
      // what a person doing the thing with a pointer and the keyboard gets, in the order it is tried
      const attempt = async (): Promise<{ heard: string[]; focused: boolean }> => {
        await page.evaluate(() => {
          ;(window as unknown as { log: string[] }).log = []
          ;(document.activeElement as HTMLElement | null)?.blur()
        })
        const point = await page.evaluate(() => {
          const el = document.getElementById('cell')!
          const inner =
            el.localName === 'ui-segmented'
              ? // a segment that is not the chosen one: choosing the chosen one is no change
                ([...el.shadowRoot!.querySelectorAll('button')].find(
                  (b) => b.getAttribute('aria-checked') !== 'true'
                ) as HTMLElement)
              : (el.shadowRoot?.querySelector('button, input, textarea, select') ?? el)
          const r = inner.getBoundingClientRect()
          return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
        })
        await page.mouse.click(point.x, point.y)
        await page.evaluate(() => document.getElementById('cell')!.click())
        const focused = await page.evaluate(() => {
          const el = document.getElementById('cell')!
          ;(document.activeElement as HTMLElement | null)?.blur()
          el.focus()
          return document.activeElement === el || !!el.contains(document.activeElement)
        })
        const heard = await page.evaluate(() => [...(window as unknown as { log: string[] }).log])
        return { heard, focused }
      }
      for (const [i, step] of route.steps.entries()) {
        await drive(step)
        const { heard, focused } = await attempt()
        const where = `${kind}, by ${route.name}, after "${step}"`
        if (route.deadAfter[i]) {
          // dead: nothing heard (not a click, not a change, not a submit) and no focus
          if (heard.length)
            wrong.push(`${where}: still disabled but heard ${JSON.stringify(heard)}`)
          if (focused) wrong.push(`${where}: still disabled but took focus`)
        } else {
          // alive: the click is heard, the control acts (a submit, a change, or the field taking focus), focus works
          const acts =
            kind === 'native' || kind === 'ui-button'
              ? 'submit'
              : TAKES_FOCUS.includes(kind)
                ? 'focusin'
                : 'change'
          if (!heard.includes('click') && !TAKES_FOCUS.includes(kind) && kind !== 'ui-segmented')
            wrong.push(`${where}: enabled again but heard no click (${JSON.stringify(heard)})`)
          if (!heard.includes(acts))
            wrong.push(`${where}: enabled again but did not act (${JSON.stringify(heard)})`)
          if (!focused) wrong.push(`${where}: enabled again but takes no focus`)
        }
      }
    }
  }
  expect(wrong).toEqual([])
})

// Four more cells of the disabled table: where a disabled control sits matters as little as how it was disabled.
test('a disabled control in a wrapping label, a disabled segment, a control in a disabled row and a disabled option row do nothing', async ({
  page
}) => {
  await page.setViewportSize({ width: 900, height: 900 })
  await page.goto(`${origin}/standalone`)
  await page.evaluate(async () => {
    const w = window as unknown as { log: string[] }
    w.log = []
    const mk = (tag: string, attrs: Record<string, string> = {}, text = ''): HTMLElement => {
      const el = document.createElement(tag)
      for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
      el.textContent = text
      return el
    }
    const listen = (el: HTMLElement, id: string): void => {
      for (const type of ['click', 'change', 'press', 'focusin'])
        el.addEventListener(type, () => w.log.push(`${id}:${type}`))
    }
    const area = mk('div')
    area.style.cssText = 'display:grid;gap:16px;padding:16px;width:600px'
    // 1. a disabled toggle inside a label that wraps it: a click on the label's own text does nothing, as for an input
    const wrap = mk('label', { id: 'wrap-label' })
    const wrapped = mk('ui-toggle', { id: 'wrapped', label: 'Sync', disabled: '' })
    listen(wrapped, 'wrapped')
    wrap.append(wrapped, ' Sync over the network')
    // 2. a segmented control with one disabled segment
    const seg = mk('ui-segmented', { id: 'seg', label: 's' }) as HTMLElement & {
      options: unknown
      value: string
    }
    seg.options = [
      { value: 'a', label: 'A' },
      { value: 'b', label: 'B', disabled: true },
      { value: 'c', label: 'C' }
    ]
    seg.value = 'a'
    listen(seg, 'seg')
    // 3. an enabled toggle in a disabled row
    const list = mk('ui-list', { label: 'l' })
    const section = mk('ui-section')
    const row = mk('ui-row', { id: 'dead-row', heading: 'Row', tappable: '', disabled: '' })
    const inRow = mk('ui-toggle', { id: 'in-row', label: 'In row', slot: 'trailing' })
    listen(row, 'row')
    listen(inRow, 'in-row')
    row.append(inRow)
    section.append(row)
    list.append(section)
    // 4. a disabled option row
    const list2 = mk('ui-list', { label: 'o', selection: 'single' })
    const section2 = mk('ui-section')
    const option = mk('ui-row', { id: 'option', heading: 'Option', option: '', disabled: '' })
    listen(option, 'option')
    section2.append(option)
    list2.append(section2)
    area.append(wrap, seg, list, list2)
    document.body.replaceChildren(area)
    await Promise.all(
      [...document.querySelectorAll('*')]
        .filter((e) => 'updateComplete' in e)
        .map((e) => (e as unknown as { updateComplete: Promise<unknown> }).updateComplete)
    )
  })
  const heard = (): Promise<string[]> =>
    page.evaluate(() => [...(window as unknown as { log: string[] }).log])
  const clear = (): Promise<void> =>
    page.evaluate(() => {
      ;(window as unknown as { log: string[] }).log = []
      ;(document.activeElement as HTMLElement | null)?.blur()
    })
  const pointOf = (js: () => Element): Promise<{ x: number; y: number }> =>
    page.evaluate((src) => {
      const r = (new Function(`return (${src})()`)() as Element).getBoundingClientRect()
      return { x: r.left + r.width / 2, y: r.top + r.height / 2 }
    }, js.toString())

  // 1. the label's text (the right end of the label, past the toggle)
  await clear()
  const labelBox = await page.locator('#wrap-label').boundingBox()
  await page.mouse.click(labelBox!.x + labelBox!.width - 8, labelBox!.y + labelBox!.height / 2)
  expect(await heard(), 'a click on the text of a label that wraps a disabled toggle').toEqual([])
  expect(
    await page.evaluate(() =>
      (document.getElementById('wrapped') as HTMLInputElement).matches(':disabled')
    )
  ).toBe(true)

  // 2. the disabled segment: a click, and the keyboard from the segment before it, never choose it
  await clear()
  const b = await pointOf(
    () => document.getElementById('seg')!.shadowRoot!.querySelectorAll('button')[1]
  )
  await page.mouse.click(b.x, b.y)
  // (the group may take focus, as a click on the empty part of a labelled group does: only an action would be wrong)
  expect(
    (await heard()).filter((e) => !e.endsWith(':focusin')),
    'a click on a disabled segment'
  ).toEqual([])
  expect(
    await page.evaluate(
      () => (document.getElementById('seg') as HTMLElement & { value: string }).value
    )
  ).toBe('a')
  await page.evaluate(() => document.getElementById('seg')!.focus())
  await page.keyboard.press('ArrowRight')
  const afterArrow = await page.evaluate(
    () => (document.getElementById('seg') as HTMLElement & { value: string }).value
  )
  expect(afterArrow, 'ArrowRight from A skips the disabled B').toBe('c')

  // 3. an enabled control in a disabled row is dead: no change, no press (NEOC-325 decides what it should look like)
  await clear()
  const t = await pointOf(() =>
    document.getElementById('in-row')!.shadowRoot!.querySelector('input, button, .track, .box')!
  )
  await page.mouse.click(t.x, t.y)
  // (a press may move focus to the toggle, as in a disabled segment's group: only an action would be wrong)
  expect(
    (await heard()).filter((e) => !e.endsWith(':focusin')),
    'a click on a toggle in a disabled row'
  ).toEqual([])

  // 4. a disabled option row takes no focus, by any call, and no tab stop
  await clear()
  const focus = await page.evaluate(() => {
    const el = document.getElementById('option') as HTMLElement & { focusRow?: () => void }
    el.focusRow?.()
    el.focus()
    return {
      focused: document.activeElement === el,
      tabindex: el.getAttribute('tabindex'),
      ariaDisabled: el.getAttribute('aria-disabled')
    }
  })
  expect(focus).toEqual({ focused: false, tabindex: null, ariaDisabled: 'true' })
  await page.keyboard.press('Enter')
  expect(await heard()).toEqual([])
})
