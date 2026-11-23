import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html, type TemplateResult } from 'lit'
import { expect, fn, waitFor, within } from 'storybook/test'
import './ui-split-view'

const meta: Meta = {
  title: 'Layout/Split view',
  component: 'ui-split-view',
  tags: ['autodocs'],
  parameters: { controls: { disable: true } }
}
export default meta
type Story = StoryObj

type Split = HTMLElement & { column: string; collapsed: boolean; updateComplete: Promise<unknown> }

const pane = (slot: string | undefined, name: string): TemplateResult =>
  html`<div
    slot=${slot ?? ''}
    data-testid=${name.toLowerCase()}
    style="padding:16px;font:var(--type-body);color:var(--label)"
  >
    ${name}
  </div>`

const view = (width: number, column: string, three = false): TemplateResult =>
  html`<div
    data-testid="window"
    style="width:${width}px;height:240px;background:var(--bg);box-shadow:0 0 0 1px var(--separator)"
  >
    <ui-split-view data-testid="split" column=${column}>
      ${pane('sidebar', 'Sidebar')} ${three ? pane('content', 'Content') : ''}
      ${pane(undefined, 'Detail')}
    </ui-split-view>
  </div>`

const shown = (canvasElement: HTMLElement): string[] =>
  ['sidebar', 'content', 'detail'].filter((id) => {
    const el = canvasElement.querySelector(`[data-testid=${id}]`)
    return el !== null && el.getClientRects().length > 0
  })

export const TwoColumns: Story = {
  render: () => view(800, 'sidebar'),
  play: async ({ canvasElement }) => {
    const split = within(canvasElement).getByTestId('split') as Split
    await expect(split.collapsed).toBe(false)
    await expect(shown(canvasElement)).toEqual(['sidebar', 'detail'])
    // the 280 pt column, less the hairline between it and the detail
    await expect(within(canvasElement).getByTestId('sidebar').getBoundingClientRect().width).toBe(
      279
    )
  }
}

export const ThreeColumns: Story = {
  render: () => view(900, 'sidebar', true),
  play: async ({ canvasElement }) => {
    await waitFor(() => expect(shown(canvasElement)).toEqual(['sidebar', 'content', 'detail']))
  }
}

export const CollapsedOnTheSidebar: Story = {
  render: () => view(480, 'sidebar'),
  play: async ({ canvasElement }) => {
    const split = within(canvasElement).getByTestId('split') as Split
    await waitFor(() => expect(split.collapsed).toBe(true))
    await expect(shown(canvasElement)).toEqual(['sidebar'])
    await expect(within(canvasElement).getByTestId('sidebar').getBoundingClientRect().width).toBe(
      480
    )
  }
}

export const CollapsedOnTheDetail: Story = {
  render: () => view(480, 'detail'),
  play: async ({ canvasElement }) => {
    const split = within(canvasElement).getByTestId('split') as Split
    await waitFor(() => expect(split.collapsed).toBe(true))
    await expect(shown(canvasElement)).toEqual(['detail'])
  }
}

/** One breakpoint: under 700 one column and an event, at 700 every column and another; `column` says which is in front. */
export const CollapsesAndComesBack: Story = {
  tags: ['play-only'],
  render: () => view(900, 'sidebar', true),
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const split = canvas.getByTestId('split') as Split
    const frame = canvas.getByTestId('window')
    const heard = fn()
    split.addEventListener('collapsedchange', (e) => heard((e as CustomEvent).detail))
    await waitFor(() => expect(shown(canvasElement)).toEqual(['sidebar', 'content', 'detail']))
    await expect(heard).not.toHaveBeenCalled()

    frame.style.width = '699px'
    await waitFor(() => expect(split.collapsed).toBe(true))
    await expect(heard).toHaveBeenLastCalledWith({ collapsed: true })
    await expect(shown(canvasElement)).toEqual(['sidebar'])
    for (const column of ['content', 'detail', 'sidebar']) {
      split.column = column
      await split.updateComplete
      await expect(shown(canvasElement)).toEqual([column])
    }

    frame.style.width = '700px'
    await waitFor(() => expect(split.collapsed).toBe(false))
    await expect(heard).toHaveBeenLastCalledWith({ collapsed: false })
    await expect(heard).toHaveBeenCalledTimes(2)
    await expect(shown(canvasElement)).toEqual(['sidebar', 'content', 'detail'])
  }
}

/** With no middle column, `content` is the detail: the nearest column there is. */
export const ContentWithoutAMiddleColumn: Story = {
  tags: ['play-only'],
  render: () => view(480, 'content'),
  play: async ({ canvasElement }) => {
    const split = within(canvasElement).getByTestId('split') as Split
    await waitFor(() => expect(split.collapsed).toBe(true))
    await expect(shown(canvasElement)).toEqual(['detail'])
  }
}
