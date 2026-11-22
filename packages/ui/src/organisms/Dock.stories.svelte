<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor } from 'storybook/test'
  import Dock from './Dock.svelte'
  import type { DockApp } from '../types'

  const { Story } = defineMeta({
    title: 'Organisms/Dock',
    component: Dock,
    tags: ['autodocs'],
    parameters: { layout: 'padded' }
  })

  const apps: DockApp[] = [
    { id: 'calculator', name: 'Calculator', icon: 'calculator', background: 'linear-gradient(180deg, #ffa733, #ff9500)', running: true },
    { id: 'files', name: 'Files', icon: 'folder', background: 'linear-gradient(180deg, #4da3ff, #0a84ff)' },
    { id: 'terminal', name: 'Terminal', icon: 'terminal', background: 'linear-gradient(180deg, #4a4a4c, #1c1c1e)' },
    { id: 'notes', name: 'Notes', icon: 'doc', background: 'linear-gradient(180deg, #ffe066, #ffcc00)' }
  ]

  /** Open, but not kept in the Dock. */
  const recents: DockApp[] = [
    { id: 'notes', name: 'Notes', icon: 'doc', background: 'linear-gradient(180deg, #ffe066, #ffcc00)', running: true },
    { id: 'agents', name: 'Agents', icon: 'sparkles', background: 'linear-gradient(180deg, #bf5af2, #5e5ce6)', running: true }
  ]
  const selected = fn()
  const asked = fn()

  const nameOf = (canvas: { getByTestId: (id: string) => HTMLElement }, id: string): HTMLElement =>
    canvas.getByTestId(`dock-${id}-name`)
</script>

{#snippet wallpaper()}
  <div
    style="display:flex;align-items:flex-end;justify-content:center;width:560px;height:200px;padding-bottom:16px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)"
  >
    <Dock testid="dock" {apps} />
  </div>
{/snippet}

<!-- An icon alone does not say what it is: hovering or tabbing to it shows its name in a small flat capsule (hidden otherwise, and from assistive technology: the button already names it). -->
<Story
  name="Names"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const calc = await canvas.findByTestId('dock-calculator')
    await expect(calc).toHaveAccessibleName('Calculator')
    await expect(nameOf(canvas, 'calculator')).toHaveAttribute('aria-hidden', 'true') // the button already names it
    for (const id of ['calculator', 'files', 'terminal', 'notes']) await expect(nameOf(canvas, id)).not.toBeVisible()

    // every icon has its name beside it for the eye; it shows on hover and keyboard focus (a real pointer and real key
    // presses are needed for those: the Dock journey, D7 in e2e/desktop.journeys.spec.ts)
    for (const app of apps) {
      await expect(nameOf(canvas, app.id)).toHaveTextContent(app.name)
      await expect(canvas.getByTestId(`dock-${app.id}`)).toHaveAccessibleName(app.name)
    }
  }}
>
  {@render wallpaper()}
</Story>

<!--
  The state the snapshot keeps: one name showing, as when the pointer rests on its icon. (The story check blurs and
  moves the pointer away before it takes the picture, so the capsule is held open here.)
-->
<Story
  name="Name showing"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const name = await canvas.findByTestId('dock-files-name')
    name.style.opacity = '1'
    await waitFor(() => expect(name).toBeVisible())
    await expect(name).toHaveTextContent('Files')
  }}
>
  {@render wallpaper()}
</Story>

{#snippet divided(available?: number)}
  <div
    data-testid="dock-screen"
    style="display:flex;align-items:flex-end;justify-content:center;width:{available ? available + 24 : 560}px;height:200px;padding-bottom:16px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)"
  >
    <Dock testid="dock" apps={apps.slice(0, 3)} {recents} {available} onselect={selected} onmenu={(id) => asked(id)} />
  </div>
{/snippet}

<!--
  The apps kept in the Dock, then a divider, then the recent apps that are open but not kept there. Every icon opens its
  app and has a context menu; the divider is a separator named for what follows it.
-->
<Story
  name="Pinned and recent apps"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    selected.mockClear()
    asked.mockClear()
    const tray = await canvas.findByTestId('dock')
    // pinned, the divider, then the recents: in that order across the tray
    const order = [...tray.querySelectorAll('[data-testid]')]
      .map((el) => el.getAttribute('data-testid')!)
      .filter((id) => !id.endsWith('-name'))
    await expect(order).toEqual(['dock-calculator', 'dock-files', 'dock-terminal', 'dock-divider', 'dock-notes', 'dock-agents'])
    const divider = canvas.getByTestId('dock-divider')
    await expect(divider).toHaveAttribute('role', 'separator')
    await expect(divider).toHaveAccessibleName('Recent apps')
    await expect(divider.getBoundingClientRect().left).toBeGreaterThan(canvas.getByTestId('dock-terminal').getBoundingClientRect().right)
    await expect(divider.getBoundingClientRect().right).toBeLessThan(canvas.getByTestId('dock-notes').getBoundingClientRect().left)

    await userEvent.click(canvas.getByTestId('dock-notes'))
    await expect(selected).toHaveBeenCalledWith('notes')
    await userEvent.pointer({ keys: '[MouseRight]', target: canvas.getByTestId('dock-files') })
    await expect(asked).toHaveBeenCalledWith('files')
    canvas.getByTestId('dock-agents').focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await expect(asked).toHaveBeenLastCalledWith('agents')
  }}
>
  {@render divided()}
</Story>

<!-- A narrow screen: the divider is one more thing in the row, and the row still fits (the gaps tighten, the icons shrink). -->
<Story
  name="Pinned and recent apps, narrow"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const tray = await canvas.findByTestId('dock')
    const screen = canvas.getByTestId('dock-screen').getBoundingClientRect()
    await expect(tray.scrollWidth).toBeLessThanOrEqual(tray.clientWidth + 1)
    const box = tray.getBoundingClientRect()
    await expect(box.left).toBeGreaterThanOrEqual(screen.left)
    await expect(box.right).toBeLessThanOrEqual(screen.right)
    await expect(canvas.getByTestId('dock-agents').getBoundingClientRect().width).toBeGreaterThanOrEqual(44)
  }}
>
  {@render divided(300)}
</Story>

<!-- Nothing kept and nothing open: there is no Dock to draw. With only recent apps there is no divider. -->
<Story
  name="Only recent apps"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    await canvas.findByTestId('dock-notes')
    await expect(canvas.queryByTestId('dock-divider')).toBeNull()
    await expect(canvas.getByTestId('dock-none').querySelector('[data-testid="dock"]')).toBeNull()
  }}
>
  <div style="display:grid;gap:12px;justify-items:center;width:560px;padding:16px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)">
    <Dock testid="dock" apps={[]} {recents} />
    <div data-testid="dock-none"><Dock testid="dock" apps={[]} recents={[]} /></div>
  </div>
</Story>

{#snippet narrow(count: number)}
  <div
    data-testid="dock-narrow"
    style="display:flex;align-items:flex-end;justify-content:center;width:360px;height:200px;padding-bottom:12px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)"
  >
    <Dock
      testid="dock"
      available={336}
      apps={Array.from({ length: count }, (_, i) => ({
        id: `a${i + 1}`,
        name: `App ${i + 1}`,
        icon: 'folder',
        background: 'linear-gradient(180deg, #4da3ff, #0a84ff)'
      }))}
    />
  </div>
{/snippet}

<!-- More apps than a 360 wide screen has room for at full size: one row, the gaps tighten and the icons shrink (never below the 44 px target) until they fit. -->
<Story
  name="Many apps, narrow"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const tray = await canvas.findByTestId('dock')
    const frame = canvas.getByTestId('dock-narrow').getBoundingClientRect()
    await expect(tray.scrollWidth).toBeLessThanOrEqual(tray.clientWidth + 1)
    const tops = new Set<number>()
    for (const id of ['a1', 'a6']) {
      const r = (await canvas.findByTestId(`dock-${id}`)).getBoundingClientRect()
      tops.add(Math.round(r.top))
      await expect(r.left).toBeGreaterThanOrEqual(frame.left)
      await expect(r.right).toBeLessThanOrEqual(frame.right)
      await expect(r.width).toBeGreaterThanOrEqual(44)
    }
    await expect(tops.size).toBe(1) // one row
  }}
>
  {@render narrow(6)}
</Story>

<!-- So many that even the smallest icons do not fit: the Dock scrolls sideways inside itself, so every app can still be reached (by the pointer, and by the keyboard: focus scrolls the icon into view). -->
<Story
  name="Too many to fit"
  asChild
  parameters={{ controls: { disable: true } }}
  play={async ({ canvas }) => {
    const tray = await canvas.findByTestId('dock')
    await expect(tray.scrollWidth).toBeGreaterThan(tray.clientWidth)
    // Tab through every icon: each one that has focus is wholly inside the tray (real key presses: the browser scrolls a focused
    // icon into view by its own rule, which can leave it half hidden at an edge)
    const icons = ['a1', 'a2', 'a3', 'a4', 'a5', 'a6', 'a7', 'a8', 'a9', 'a10']
    await userEvent.tab()
    for (const id of icons) {
      const icon = await canvas.findByTestId(`dock-${id}`)
      await expect(icon).toHaveFocus()
      const t = tray.getBoundingClientRect()
      const r = icon.getBoundingClientRect()
      await expect(r.left).toBeGreaterThanOrEqual(t.left - 0.5)
      await expect(r.right).toBeLessThanOrEqual(t.right + 0.5)
      await expect(r.width).toBeGreaterThanOrEqual(44)
      await userEvent.tab()
    }
  }}
>
  {@render narrow(10)}
</Story>
