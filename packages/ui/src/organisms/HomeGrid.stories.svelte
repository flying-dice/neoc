<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor } from 'storybook/test'
  import HomeGrid from './HomeGrid.svelte'
  import type { DockApp } from '../types'
  import type { IconName } from '../icons'

  const { Story } = defineMeta({
    title: 'Organisms/HomeGrid',
    component: HomeGrid,
    tags: ['autodocs'],
    parameters: { layout: 'padded', controls: { disable: true } }
  })

  const app = (id: string, name: string, icon: IconName, from: string, to: string): DockApp => ({
    id,
    name,
    icon,
    background: `linear-gradient(180deg, ${from}, ${to})`
  })

  const apps: DockApp[] = [
    app('discover', 'Discover', 'star', '#4fc3ff', '#0a6cff'),
    app('files', 'Files', 'folder', '#5ac8fa', '#007aff'),
    app('settings', 'Settings', 'gear', '#a1a1aa', '#52525b'),
    app('agents', 'Agents', 'sparkles', '#bf5af2', '#5e5ce6'),
    app('calculator', 'Calculator', 'calculator', '#ffa733', '#ff9500'),
    app('notes', 'Notes', 'doc', '#ffe066', '#ffcc00'),
    app('terminal', 'Terminal', 'terminal', '#4a4a4c', '#1c1c1e')
  ]
  /** More apps than the box has rows for. */
  const many: DockApp[] = Array.from({ length: 30 }, (_, i) =>
    app(`a${i + 1}`, i === 4 ? 'An app with a very long name' : `App ${i + 1}`, 'square.grid', '#5ac8fa', '#007aff')
  )

  const opened = fn()
  const asked = fn()

  type Canvas = { findByTestId: (id: string) => Promise<HTMLElement>; getByTestId: (id: string) => HTMLElement }
  const icon = (canvas: Canvas, id: string): HTMLElement => canvas.getByTestId(`home-${id}`)
  /** How many icons share the first one's row. */
  const columnsOf = (canvas: Canvas, list: DockApp[]): number => {
    const top = icon(canvas, list[0].id).getBoundingClientRect().top
    return list.filter((a) => Math.abs(icon(canvas, a.id).getBoundingClientRect().top - top) < 1).length
  }
</script>

{#snippet screen(list: DockApp[], width: number, height: number)}
  <div class="screen" data-testid="screen" style:width="{width}px" style:height="{height}px">
    <HomeGrid testid="home" apps={list} onselect={opened} onmenu={(id) => asked(id)} />
  </div>
{/snippet}

<!-- Each app is its icon over its name, in the order given. The icon is a button named by the app; the name under it is for the eye. -->
<Story
  name="Apps"
  asChild
  play={async ({ canvas }) => {
    opened.mockClear()
    const list = await canvas.findByTestId('home')
    await expect(list).toHaveAccessibleName('Apps')
    await expect(list.querySelectorAll('li')).toHaveLength(apps.length)
    for (const a of apps) {
      await expect(icon(canvas, a.id)).toHaveAccessibleName(a.name)
      await expect(canvas.getByTestId(`home-${a.id}-name`)).toHaveTextContent(a.name)
      await expect(canvas.getByTestId(`home-${a.id}-name`)).toHaveAttribute('aria-hidden', 'true')
    }
    // in the order given, reading across
    const lefts = apps.map((a) => icon(canvas, a.id).getBoundingClientRect().left)
    await expect([...lefts].sort((x, y) => x - y)).toEqual(lefts)
    await userEvent.click(icon(canvas, 'calculator'))
    await expect(opened).toHaveBeenCalledWith('calculator')
  }}
>
  {@render screen(apps, 720, 300)}
</Story>

<!--
  The keyboard: the grid is one stop in the tab order; the arrows move across it as across a grid of icons (Left and
  Right in reading order, Up and Down within the column), Home and End go to the ends of the row and with Ctrl to the
  first and last app; Enter opens, Shift+F10 asks for the icon's menu.
-->
<Story
  name="Keyboard"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    opened.mockClear()
    asked.mockClear()
    await canvas.findByTestId('home')
    const columns = columnsOf(canvas, many)
    await expect(columns).toBe(4)
    const at = (n: number): HTMLElement => icon(canvas, `a${n}`)
    await userEvent.tab()
    await expect(at(1)).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}{ArrowRight}')
    await expect(at(3)).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}')
    await expect(at(3 + columns)).toHaveFocus()
    await userEvent.keyboard('{ArrowLeft}')
    await expect(at(2 + columns)).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}')
    await expect(at(2)).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}') // the first row: stays
    await expect(at(2)).toHaveFocus()
    await userEvent.keyboard('{End}')
    await expect(at(columns)).toHaveFocus()
    await userEvent.keyboard('{ArrowRight}') // off the end of a row onto the next
    await expect(at(columns + 1)).toHaveFocus()
    await userEvent.keyboard('{Home}')
    await expect(at(columns + 1)).toHaveFocus()

    // one tab stop: the icon last focused; Tab leaves the grid
    await expect(at(columns + 1)).toHaveAttribute('tabindex', '0')
    await expect(at(1)).toHaveAttribute('tabindex', '-1')
    await userEvent.tab()
    await expect(canvas.getByTestId('home').contains(document.activeElement)).toBe(false)
    await userEvent.tab({ shift: true })
    await expect(at(columns + 1)).toHaveFocus()

    // the last app is below the fold: the keyboard brings it into view
    await userEvent.keyboard('{Control>}{End}{/Control}')
    await expect(at(30)).toHaveFocus()
    const box = canvas.getByTestId('home').getBoundingClientRect()
    await waitFor(() => expect(at(30).getBoundingClientRect().bottom).toBeLessThanOrEqual(box.bottom))
    await userEvent.keyboard('{Control>}{Home}{/Control}')
    await expect(at(1)).toHaveFocus()

    await userEvent.keyboard('{Enter}')
    await expect(opened).toHaveBeenCalledWith('a1')
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await expect(asked).toHaveBeenCalledWith('a1')
  }}
>
  {@render screen(many, 390, 420)}
</Story>

<!-- More apps than fit: the grid scrolls down inside itself, and nothing is past its sides. A long name is cut with an ellipsis. -->
<Story
  name="More than fit"
  asChild
  play={async ({ canvas }) => {
    const list = await canvas.findByTestId('home')
    await expect(list.scrollHeight).toBeGreaterThan(list.clientHeight)
    await expect(list.scrollWidth).toBeLessThanOrEqual(list.clientWidth)
    const long = canvas.getByTestId('home-a5-name')
    await expect(long.scrollWidth).toBeGreaterThan(long.clientWidth) // cut, not spilling over its neighbours
    await expect(long.getBoundingClientRect().width).toBeLessThanOrEqual(long.parentElement!.getBoundingClientRect().width)
  }}
>
  {@render screen(many, 720, 320)}
</Story>

<!-- A phone: four columns, every icon and name inside the screen. -->
<Story
  name="Phone width"
  asChild
  play={async ({ canvas }) => {
    await canvas.findByTestId('home')
    await expect(columnsOf(canvas, apps)).toBe(4)
    const screen = canvas.getByTestId('screen').getBoundingClientRect()
    for (const a of apps) {
      for (const el of [icon(canvas, a.id), canvas.getByTestId(`home-${a.id}-name`)]) {
        const r = el.getBoundingClientRect()
        await expect(r.left).toBeGreaterThanOrEqual(screen.left)
        await expect(r.right).toBeLessThanOrEqual(screen.right)
      }
    }
  }}
>
  {@render screen(apps, 390, 420)}
</Story>

<style>
  .screen {
    display: flex;
    justify-content: center;
    padding: 16px;
    box-sizing: border-box;
    border-radius: var(--radius-xl);
    background:
      radial-gradient(120% 90% at 10% 0%, #6e8efb 0%, transparent 55%),
      radial-gradient(90% 80% at 100% 10%, #c471ed 0%, transparent 55%),
      radial-gradient(100% 90% at 80% 100%, #f7797d 0%, transparent 55%),
      radial-gradient(90% 90% at 0% 100%, #12c2e9 0%, transparent 60%), #2a2d6b;
  }
</style>
