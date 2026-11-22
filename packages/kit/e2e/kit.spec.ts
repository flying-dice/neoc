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

test('with hit, every control has a hit area of at least 44 x 44 pt around the drawn control', async ({
  page
}) => {
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
  test(`a click on the empty part of a stretched element does nothing (hit ${hit ? 'on' : 'off'})`, async ({
    page
  }) => {
    await page.goto(`${origin}/frame`)
    // A control activates only when the click lands on the drawn control (or its 44 pt box with hit), or comes through a
    // label, never because its element is larger (a flex column or a grid cell stretches it, and so does a taller neighbour
    // in a flex row). Click just outside the drawn box (hit off) or the 44 pt box (hit on), inside the element.
    const points = await page.evaluate(async (hit) => {
      const w = window as unknown as { log: string[] }
      w.log = []
      const kinds: [string, Record<string, string>, string][] = [
        ['ui-button', { variant: 'tinted', size: 'small', type: 'button' }, 'Go'],
        ['ui-toggle', { label: 't' }, ''],
        ['ui-check', { label: 'c' }, ''],
        ['ui-segmented', { label: 's', value: 'a' }, ''],
        ['ui-text-field', { label: 'f' }, '']
      ]
      const layouts: [string, string, string[]][] = [
        ['column', 'display:flex;flex-direction:column;align-items:stretch;width:300px', ['right']],
        [
          'grid',
          'display:grid;grid-template-columns:300px;grid-auto-rows:110px',
          ['right', 'below']
        ],
        ['row', 'display:flex;align-items:stretch;gap:12px', ['below', 'above']]
      ]
      const area = document.createElement('div')
      area.style.cssText = 'display:grid;gap:24px;padding:24px'
      const items: { id: string; el: HTMLElement; sides: string[] }[] = []
      for (const [layout, css, sides] of layouts)
        for (const [tag, attrs, text] of kinds) {
          const box = document.createElement('div')
          box.style.cssText = css
          const el = document.createElement(tag)
          for (const [k, v] of Object.entries(attrs)) el.setAttribute(k, v)
          if (hit) el.setAttribute('hit', '')
          el.textContent = text
          const id = `${layout}-${tag}`
          el.dataset.testid = id
          if (tag === 'ui-segmented')
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
          for (const type of ['click', 'change', 'focusin'])
            el.addEventListener(type, () => w.log.push(`${id}:${type}`))
          items.push({ id, el, sides })
        }
      document.body.replaceChildren(area)
      await Promise.all(
        items.map((i) => (i.el as unknown as { updateComplete: Promise<unknown> }).updateComplete)
      )
      const out: { id: string; side: string; x: number; y: number }[] = []
      for (const { id, el, sides } of items) {
        const host = el.getBoundingClientRect()
        const inner = el
          .shadowRoot!.querySelector<HTMLElement>(
            el.localName === 'ui-segmented'
              ? '.group'
              : el.localName === 'ui-text-field'
                ? 'input'
                : 'button'
          )!
          .getBoundingClientRect()
        const cx = inner.left + inner.width / 2
        const cy = inner.top + inner.height / 2
        // the area that may take a click: the drawn box, or the 44 pt box centred on it (a segmented control is only taller)
        const halfW =
          hit && el.localName !== 'ui-segmented' && el.localName !== 'ui-text-field'
            ? Math.max(inner.width, 44) / 2
            : inner.width / 2
        const halfH = hit ? Math.max(inner.height, 44) / 2 : inner.height / 2
        const spots: Record<string, [number, number]> = {
          right: [cx + halfW + 8, cy],
          below: [cx, cy + halfH + 6],
          above: [cx, cy - halfH - 6]
        }
        for (const side of sides) {
          const [x, y] = spots[side]!
          // only where the point is inside the (stretched) element, so that the element is what is being clicked
          if (x > host.left && x < host.right && y > host.top && y < host.bottom)
            out.push({ id, side, x, y })
        }
      }
      return out
    }, hit)
    expect(points.length, 'the layouts stretch most of the controls').toBeGreaterThan(10)
    const wrong: string[] = []
    for (const p of points) {
      await page.evaluate(() => {
        ;(window as unknown as { log: string[] }).log = []
        ;(document.activeElement as HTMLElement | null)?.blur()
      })
      await page.mouse.click(p.x, p.y)
      const log = await page.evaluate(() => (window as unknown as { log: string[] }).log)
      if (log.length) wrong.push(`${p.id} ${p.side}: ${JSON.stringify(log)}`)
    }
    expect(wrong).toEqual([])
  })
}
