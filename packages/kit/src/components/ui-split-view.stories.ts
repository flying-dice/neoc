import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html, type TemplateResult } from 'lit'
import { expect, fn, waitFor, within } from 'storybook/test'
import './ui-split-view'
import './ui-button'
import './ui-icon'
import './ui-list'

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

/**
 * One breakpoint: under 700 one column and an event, at 700 every column and another; `column` says which is in
 * front.
 */
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

/**
 * The way back is the app's, in the header of what it shows: it is drawn while the split view is `collapsed`, and sets
 * `column`. Picking a row shows the detail; Back shows the sidebar again. Each time the focus goes with the column.
 */
export const AWayBack: Story = {
  render: () => {
    const show = (e: Event, column: string): void => {
      ;((e.currentTarget as HTMLElement).closest('ui-split-view') as Split).column = column
    }
    const collapsed = (e: Event): void => {
      const split = e.currentTarget as Split
      split.querySelector<HTMLElement>('[data-testid=back]')!.hidden = !split.collapsed
    }
    return html`<div
      data-testid="window"
      style="width:480px;height:240px;background:var(--bg);box-shadow:0 0 0 1px var(--separator)"
    >
      <ui-split-view data-testid="split" column="detail" @collapsedchange=${collapsed}>
        <div slot="sidebar" data-testid="sidebar">
          <ui-list label="Lists" variant="sidebar">
            <ui-section>
              <ui-row
                data-testid="row"
                heading="Reminders"
                tappable
                @press=${(e: Event) => show(e, 'detail')}
              ></ui-row>
            </ui-section>
          </ui-list>
        </div>
        <div data-testid="detail" style="padding:8px 16px;color:var(--label)">
          <header style="display:flex;align-items:center;gap:8px;min-height:44px">
            <ui-button
              data-testid="back"
              hidden
              variant="plain"
              size="small"
              hit
              label="Back to the lists"
              @click=${(e: Event) => show(e, 'sidebar')}
            >
              <ui-icon slot="icon" name="chevron.left"></ui-icon>
              Lists
            </ui-button>
            <h1 style="margin:0;font:var(--type-title2);font-weight:700">Reminders</h1>
          </header>
        </div>
      </ui-split-view>
    </div>`
  },
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const split = canvas.getByTestId('split') as Split
    await waitFor(() => expect(split.collapsed).toBe(true))
    await expect(shown(canvasElement)).toEqual(['detail'])
    const back = canvas.getByTestId('back')
    await waitFor(() => expect(back).toBeVisible())
    // a real press leaves the focus on what was pressed; the test library's click takes it off a kit element, so the
    // story focuses and presses by hand (the kit's e2e spec does both with a real pointer)
    back.focus()
    back.click()
    await split.updateComplete
    await expect(shown(canvasElement)).toEqual(['sidebar'])
    await expect(canvas.getByTestId('sidebar').matches(':focus-within')).toBe(true)
    const row = canvas.getByTestId('row') as HTMLElementTagNameMap['ui-row']
    row.focusRow()
    row.shadowRoot!.querySelector<HTMLElement>('.hit')!.click()
    await split.updateComplete
    await expect(shown(canvasElement)).toEqual(['detail'])
    await expect(canvas.getByTestId('detail').matches(':focus-within')).toBe(true)

    // with room for both columns there is nowhere to go back to: no Back
    canvas.getByTestId('window').style.width = '760px'
    await waitFor(() => expect(split.collapsed).toBe(false))
    await expect(back).not.toBeVisible()
    await expect(shown(canvasElement)).toEqual(['sidebar', 'detail'])
  }
}
