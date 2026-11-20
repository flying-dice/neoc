<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
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
    { id: 'terminal', name: 'Terminal', icon: 'terminal', background: 'linear-gradient(180deg, #4a4a4c, #1c1c1e)', badge: 3 }
  ]
  const recents: DockApp[] = [
    { id: 'notes', name: 'Notes', icon: 'doc', background: 'linear-gradient(180deg, #ffe066, #ffcc00)' }
  ]

  const nameOf = (canvas: { getByTestId: (id: string) => HTMLElement }, id: string): HTMLElement =>
    canvas.getByTestId(`dock-${id}-name`)
</script>

{#snippet wallpaper()}
  <div
    style="display:flex;align-items:flex-end;justify-content:center;width:560px;height:200px;padding-bottom:16px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)"
  >
    <Dock testid="dock" {apps} {recents} />
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

    // every icon, pinned or recent, has its name beside it for the eye; it shows on hover and keyboard focus (a real
    // pointer and real key presses are needed for those: the Dock journey, D7 in e2e/desktop.journeys.spec.ts)
    for (const app of [...apps, ...recents]) {
      await expect(nameOf(canvas, app.id)).toHaveTextContent(app.name)
      await expect(canvas.getByTestId(`dock-${app.id}`)).toHaveAccessibleName(
        app.badge ? `${app.name}, ${app.badge} new` : app.name
      )
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
