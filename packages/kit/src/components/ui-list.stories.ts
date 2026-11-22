import type { Meta, StoryObj } from '@storybook/web-components-vite'
import { html, type TemplateResult } from 'lit'
import { expect, fn, userEvent, within } from 'storybook/test'

import './ui-icon'
import './ui-list'
import './ui-toggle'
import './ui-check'
import './ui-button'

const meta: Meta = {
  title: 'Lists/List',
  component: 'ui-list',
  tags: ['autodocs'],
  parameters: { controls: { disable: true }, layout: 'padded' }
}
export default meta
type Story = StoryObj

const settings = (variant: string): TemplateResult =>
  html`<ui-list data-testid="list-${variant}" variant=${variant} label="Settings">
    <ui-section header="Network" footer="Wi-Fi is used when it is available.">
      <ui-row
        data-testid="row-wifi"
        heading="Wi-Fi"
        icon="wifi"
        icon-color="blue"
        value="Home"
        chevron
        tappable
      ></ui-row>
      <ui-row data-testid="row-vpn" heading="VPN" icon="lock" icon-color="indigo"
        ><ui-toggle slot="trailing" data-testid="toggle-vpn" label="VPN"></ui-toggle
      ></ui-row>
      <ui-row
        data-testid="row-notify"
        heading="Notifications"
        subheading="Banners, sounds"
        icon="bell"
        icon-color="red"
        chevron
        tappable
      ></ui-row>
    </ui-section>
    <ui-section header="Account">
      <ui-row data-testid="row-add" heading="Add Account…" action tappable></ui-row>
      <ui-row data-testid="row-signout" heading="Sign Out" destructive tappable></ui-row>
    </ui-section>
  </ui-list>`

export const InsetGrouped: Story = {
  render: () => html`<div style="width:420px">${settings('insetGrouped')}</div>`
}
export const Inset: Story = {
  render: () => html`<div style="width:420px">${settings('inset')}</div>`
}
export const Plain: Story = {
  render: () => html`<div style="width:420px">${settings('plain')}</div>`
}
export const Grouped: Story = {
  render: () => html`<div style="width:420px">${settings('grouped')}</div>`
}
export const Sidebar: Story = {
  render: () =>
    html`<div style="width:300px">
      <ui-list data-testid="sidebar" variant="sidebar" selection="single" label="Chats">
        <ui-section header="Chats">
          <ui-row
            data-testid="chat-1"
            heading="Weekend plans"
            subheading="Ada  2 min ago"
            icon="message"
            selected
          ></ui-row>
          <ui-row
            data-testid="chat-2"
            heading="Budget review"
            subheading="Grace  yesterday"
            icon="message"
          ></ui-row>
          <ui-row
            data-testid="chat-3"
            heading="Release notes"
            subheading="Linus  Mon"
            icon="message"
          ></ui-row>
        </ui-section>
      </ui-list>
    </div>`
}
export const SingleSelection: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="single" selection="single" label="Sort by">
        <ui-section header="Sort by">
          <ui-row data-testid="sort-name" heading="Name" selected></ui-row>
          <ui-row data-testid="sort-date" heading="Date modified"></ui-row>
          <ui-row data-testid="sort-size" heading="Size"></ui-row>
        </ui-section>
      </ui-list>
    </div>`
}
export const MultipleSelection: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="multi" selection="multi" label="Days">
        <ui-section header="Repeat on">
          <ui-row data-testid="day-mon" heading="Monday" selected></ui-row>
          <ui-row data-testid="day-tue" heading="Tuesday"></ui-row>
          <ui-row data-testid="day-wed" heading="Wednesday" selected></ui-row>
        </ui-section>
      </ui-list>
    </div>`
}
export const Rows: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="rows" label="Tasks">
        <ui-section header="Tasks">
          <ui-row data-testid="task-1" heading="Buy milk"
            ><ui-check slot="leading" data-testid="check-1" label="Buy milk done"></ui-check
          ></ui-row>
          <ui-row
            data-testid="task-2"
            heading="A very long title that does not fit on one line of the row and so is cut short"
            subheading="And a subtitle that also runs on for far too long to fit"
          ></ui-row>
          <ui-row data-testid="task-3" heading="Disabled" disabled tappable></ui-row>
          <ui-row data-testid="task-4"
            ><ui-button data-testid="task-4-button" variant="tinted" size="small"
              >Custom content</ui-button
            ></ui-row
          >
          <ui-row data-testid="task-5"
            ><span style="min-width:90px">Name</span
            ><ui-text-field
              data-testid="task-5-field"
              variant="bare"
              label="Name"
              placeholder="Required"
            ></ui-text-field
          ></ui-row>
        </ui-section>
      </ui-list>
    </div>`
}

export const Navigating: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="nav" label="Options">
        <ui-section header="Options">
          <ui-row data-testid="nav-a" heading="First" tappable></ui-row>
          <ui-row data-testid="nav-b" heading="Second" tappable></ui-row>
          <ui-row data-testid="nav-c" heading="Third" tappable></ui-row>
        </ui-section>
      </ui-list>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const a = canvas.getByTestId('nav-a')
    const stop = (id: string): string | null | undefined =>
      canvas.getByTestId(id).shadowRoot?.querySelector('.hit')?.getAttribute('tabindex')
    const onTap = fn()
    canvasElement.addEventListener('press', (e) => onTap((e.target as HTMLElement).dataset.testid))
    // one tab stop: only the first row is in the tab order; the arrow keys walk the others
    await expect([stop('nav-a'), stop('nav-b'), stop('nav-c')]).toEqual(['0', '-1', '-1'])
    ;(a as HTMLElement & { focusRow(): void }).focusRow()
    await expect(document.activeElement).toBe(a)
    await userEvent.keyboard('{ArrowDown}')
    await expect(document.activeElement).toBe(canvas.getByTestId('nav-b'))
    await userEvent.keyboard('{End}')
    await expect(document.activeElement).toBe(canvas.getByTestId('nav-c'))
    await userEvent.keyboard('{Home}')
    await expect(document.activeElement).toBe(a)
    await userEvent.keyboard('{Enter}')
    await expect(onTap).toHaveBeenLastCalledWith('nav-a')
    // the row you moved to is the one you come back to
    await userEvent.keyboard('{ArrowDown}')
    await expect([stop('nav-a'), stop('nav-b'), stop('nav-c')]).toEqual(['-1', '0', '-1'])
  }
}

export const SelectingByKeyboard: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="pick" selection="single" label="Sort by">
        <ui-section header="Sort by">
          <ui-row data-testid="pick-a" heading="Name" selected></ui-row>
          <ui-row data-testid="pick-b" heading="Date"></ui-row>
        </ui-section>
      </ui-list>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const onTap = fn()
    const b = canvas.getByTestId('pick-b')
    b.addEventListener('press', onTap)
    await expect(canvas.getByTestId('pick-a')).toHaveAttribute('tabindex', '0') // the selected row is the tab stop
    await expect(b).toHaveAttribute('tabindex', '-1')
    canvas.getByTestId('pick-a').focus()
    await expect(document.activeElement).toBe(canvas.getByTestId('pick-a'))
    await userEvent.keyboard('{ArrowDown}')
    await expect(document.activeElement).toBe(b)
    await userEvent.keyboard(' ')
    await expect(onTap).toHaveBeenCalledTimes(1)
  }
}

export const TrailingControlInATappableRow: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="trailing" label="Settings">
        <ui-section header="Network">
          <ui-row data-testid="trailing-row" heading="VPN" tappable
            ><ui-toggle slot="trailing" data-testid="trailing-toggle" label="VPN"></ui-toggle
          ></ui-row>
        </ui-section>
      </ui-list>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const row = canvas.getByTestId('trailing-row')
    const toggle = canvas.getByTestId('trailing-toggle') as HTMLElement & { checked: boolean }
    const onPress = fn()
    row.addEventListener('press', onPress)
    // flipping the toggle is the toggle's business: the row is not pressed
    await userEvent.click(toggle)
    await expect(toggle.checked).toBe(true)
    await expect(onPress).not.toHaveBeenCalled()
    // tapping the row is a press
    row.shadowRoot!.querySelector<HTMLElement>('.hit')!.click()
    await expect(onPress).toHaveBeenCalledTimes(1)
    await expect(toggle.checked).toBe(true)
  }
}

export const LeadingControlInATappableRow: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="leading" label="Tasks">
        <ui-section header="Today">
          <ui-row
            data-testid="leading-row"
            heading="Buy milk"
            subheading="Two litres"
            tappable
            chevron
            ><ui-check slot="leading" data-testid="leading-check" label="Buy milk done"></ui-check
          ></ui-row>
          <ui-row data-testid="leading-button-row" heading="Archive" tappable
            ><ui-button
              slot="leading"
              data-testid="leading-button"
              variant="tinted"
              size="small"
              square
              label="Archive now"
              ><ui-icon name="folder" size="15"></ui-icon></ui-button
          ></ui-row>
        </ui-section>
      </ui-list>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const row = canvas.getByTestId('leading-row')
    const check = canvas.getByTestId('leading-check') as HTMLElement & { checked: boolean }
    const onPress = fn()
    row.addEventListener('press', onPress)
    // a leading control is above the row's tap overlay, as a trailing one is (a simulated click would not notice)
    const topAt = (el: HTMLElement): Element | null => {
      const r = el.getBoundingClientRect()
      return document.elementFromPoint(r.left + r.width / 2, r.top + r.height / 2)
    }
    await expect(topAt(check)).toBe(check)
    // a leading control is clickable and does not press the row
    await userEvent.click(check)
    await expect(check.checked).toBe(true)
    await expect(onPress).not.toHaveBeenCalled()
    const onClick = fn()
    const button = canvas.getByTestId('leading-button')
    button.addEventListener('click', onClick)
    await expect(topAt(button)).toBe(button)
    const buttonRow = canvas.getByTestId('leading-button-row')
    const onButtonRowPress = fn()
    buttonRow.addEventListener('press', onButtonRowPress)
    await userEvent.click(button)
    await expect(onClick).toHaveBeenCalledTimes(1)
    await expect(onButtonRowPress).not.toHaveBeenCalled()
    // and a row's own tap is still a press
    row.shadowRoot!.querySelector<HTMLElement>('.hit')!.click()
    await expect(onPress).toHaveBeenCalledTimes(1)
  }
}

export const ControlsSideBySideInARow: Story = {
  render: () =>
    html`<div style="width:420px">
      <ui-list data-testid="side-by-side" label="Controls">
        <ui-section header="Side by side">
          <ui-row data-testid="side-checks" heading="Two checks" tappable
            ><ui-check slot="trailing" data-testid="side-check-1" label="First"></ui-check
            ><ui-check slot="trailing" data-testid="side-check-2" label="Second"></ui-check
          ></ui-row>
          <ui-row data-testid="side-icon" heading="An icon button and a check" tappable
            ><ui-button
              slot="trailing"
              data-testid="side-icon-button"
              variant="plain"
              size="small"
              square
              label="Info"
              ><ui-icon name="info.circle" size="18"></ui-icon></ui-button
            ><ui-check slot="trailing" data-testid="side-check-3" label="Third"></ui-check
          ></ui-row>
        </ui-section>
      </ui-list>
    </div>`,
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    // a row is as tall as its content: a 24 pt check (46) or a 28 pt icon button (50) and the 11 pt padding above and below it, as with
    // any control of that size in the shell's row, and neighbouring controls' boxes never overlap
    const box = (id: string): DOMRect => canvas.getByTestId(id).getBoundingClientRect()
    await expect(box('side-checks').height).toBe(46)
    await expect(box('side-icon').height).toBe(50) // a 28 pt icon button
    for (const [a, b] of [
      ['side-check-1', 'side-check-2'],
      ['side-icon-button', 'side-check-3']
    ] as const)
      await expect(box(a).right).toBeLessThanOrEqual(box(b).left + 0.01)
  }
}
