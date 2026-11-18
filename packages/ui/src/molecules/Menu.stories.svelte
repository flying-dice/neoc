<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Menu from './Menu.svelte'
  import type { MenuItem } from '../types'

  const { Story } = defineMeta({
    title: 'Molecules/Menu',
    component: Menu,
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })

  const run = fn()
  const close = fn()

  const items: MenuItem[] = [
    { label: 'New note', icon: 'square.and.pencil', shortcut: '⌘N', onclick: () => run('new') },
    { label: 'Duplicate', icon: 'doc', onclick: () => run('duplicate') },
    { separator: true },
    { heading: 'Sort by' },
    { label: 'Title', checked: true, onclick: () => run('title') },
    { label: 'Date edited', checked: false, onclick: () => run('date') },
    { separator: true },
    {
      label: 'Move to',
      icon: 'folder',
      submenu: [
        { label: 'Inbox', onclick: () => run('inbox') },
        { label: 'Archive', onclick: () => run('archive') },
        { label: 'Locked', disabled: true }
      ]
    },
    { label: 'Delete', icon: 'trash', destructive: true, onclick: () => run('delete') }
  ]
</script>

<Story name="iPadOS menu" asChild>
  <Menu testid="menu" label="Actions" variant="ios" {items} />
</Story>

<Story name="macOS menu" asChild>
  <Menu testid="menu" label="Actions" variant="mac" {items} />
</Story>

<Story
  name="Keyboard: arrows, type-ahead, submenu, escape"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const item = (key: string): HTMLElement => canvas.getByTestId(`menu-${key}`)
    await waitFor(() => expect(item('new-note')).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}')
    await expect(item('duplicate')).toHaveFocus()
    await userEvent.keyboard('{ArrowDown}') // the separator and heading are skipped
    await expect(item('title')).toHaveFocus()
    await userEvent.keyboard('d') // type-ahead to "Date edited"
    await expect(item('date-edited')).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}{ArrowUp}{ArrowUp}{End}')
    await expect(item('delete')).toHaveFocus()
    await userEvent.keyboard('{ArrowUp}{ArrowRight}') // Move to, open
    await waitFor(() => expect(item('inbox')).toHaveFocus())
    await userEvent.keyboard('{ArrowDown}{Enter}')
    await waitFor(() => expect(run).toHaveBeenCalledWith('archive'))
    await userEvent.keyboard('{Escape}') // closes the submenu first
    await waitFor(() => expect(canvas.queryByTestId('menu-inbox')).toBeNull())
    await expect(item('move-to')).toHaveFocus()
    await userEvent.keyboard('{Escape}') // then the menu
    await waitFor(() => expect(close).toHaveBeenCalled())
  }}
>
  <Menu testid="menu" label="Actions" variant="ios" {items} autofocus onselect={() => undefined} onclose={close} />
</Story>

<Story
  name="Submenu closes with Left"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.click(canvas.getByTestId('menu-move-to'))
    await waitFor(() => expect(canvas.getByTestId('menu-inbox')).toBeVisible())
    await userEvent.keyboard('{ArrowDown}')
    await userEvent.keyboard('{ArrowLeft}')
    await waitFor(() => expect(canvas.queryByTestId('menu-inbox')).toBeNull())
    await expect(canvas.getByTestId('menu-move-to')).toHaveFocus()
  }}
>
  <Menu testid="menu" label="Actions" variant="ios" {items} />
</Story>

<Story name="Mac submenu flyout" asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.hover(canvas.getByTestId('menu-move-to'))
    await waitFor(() => expect(canvas.getByTestId('menu-inbox')).toBeVisible())
  }}
>
  <div style="padding:0 230px 90px 0"><Menu testid="menu" label="Actions" variant="mac" {items} /></div>
</Story>

<Story name="Checkmark options" asChild>
  <Menu
    testid="menu"
    label="View"
    variant="ios"
    items={[
      { label: 'Show completed', checked: true },
      { label: 'Show subtasks', checked: false },
      { separator: true },
      { label: 'Compact rows', checked: true }
    ]}
  />
</Story>
