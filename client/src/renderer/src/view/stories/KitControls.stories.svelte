<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fireEvent, waitFor } from 'storybook/test'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import ViewHost from '../organisms/ViewHost.svelte'
  import { openKitStory, type KitStoryHandle } from '../host/kitStory'
  import type { KitAppOptions } from '../host/kitApp'
  import { until } from '../host/storyKit'
  import * as trees from '../host/kitTrees'

  const scenarios: Record<string, KitAppOptions> = {
    controls: trees.controlsForm,
    display: trees.display,
    editor: trees.editor,
    presentations: trees.presentations,
    menus: trees.menus,
    markdown: trees.markdown,
    dnd: trees.dnd,
    toast: trees.toast
  }

  /** The session of the story on screen, for its play function (the host side of the scenario). */
  let latest: Promise<KitStoryHandle> | null = null
  const open = (name: string): Promise<KitStoryHandle> => {
    void latest?.then((previous) => previous.dispose()) // a re-render replaces the session; do not leak the old one
    return (latest = openKitStory(scenarios[name]!))
  }
  const story = async (): Promise<KitStoryHandle> => {
    await until(() => latest !== null, 'the story session')
    return latest!
  }

  /**
   * The managed renderer's tier-1 kit: controls, presentation and display components rendered from JSON view trees
   * through the registry, on the fake host. Every story drives the host side from its play function.
   */
  const { Story } = defineMeta({
    title: 'Renderer/Kit - controls',
    component: ViewHost,
    tags: ['app'],
    args: { width: 480, height: 720, scenario: 'controls' as string },
    argTypes: { scenario: { control: false } },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number; scenario: string })}
  <AppWindow title="Kit" width={args.width} height={args.height} minWidth={320} minHeight={240}>
    {#await open(args.scenario)}
      <p class="loading">Connecting…</p>
    {:then s}
      <div class="fill" {@attach () => () => s.dispose()}>
        <ViewHost view={s.view} app="kit" onopenurl={(url) => s.links.push(url)} />
      </div>
    {/await}
  </AppWindow>
{/snippet}

<!-- A form of every control, two-way bound: each change shows in the readout at once and reaches the host. -->
<Story
  name="Form of controls"
  play={async ({ canvas, userEvent }) => {
    const summary = await canvas.findByTestId('kit-summary')
    await waitFor(() =>
      expect(summary).toHaveTextContent('Ada | 0.4 | 3 | title | 2026-11-18 | blue')
    )

    const name = canvas.getByTestId('kit-name')
    await userEvent.clear(name)
    await userEvent.type(name, 'Grace')
    await waitFor(() => expect(summary).toHaveTextContent('Grace | 0.4'))

    await userEvent.click(canvas.getByTestId('kit-count-increment'))
    await waitFor(() => expect(summary).toHaveTextContent('| 4 |'))

    await userEvent.click(canvas.getByTestId('kit-sort-seg-date'))
    await waitFor(() => expect(summary).toHaveTextContent('| date |'))
    await expect(canvas.getByTestId('kit-sort-inline-option-date')).toHaveAttribute(
      'aria-selected',
      'true'
    )

    await userEvent.click(canvas.getByTestId('kit-color-swatch-teal'))
    await waitFor(() => expect(summary).toHaveTextContent('| teal'))

    await fireEvent.change(canvas.getByTestId('kit-when-input'), {
      target: { value: '2026-12-01' }
    })
    await waitFor(() => expect(summary).toHaveTextContent('| 2026-12-01 |'))

    await fireEvent.input(canvas.getByTestId('kit-volume'), { target: { value: '0.8' } })
    await waitFor(() => expect(summary).toHaveTextContent('| 0.8 |'))

    const { host } = await story()
    await until(() => host.stateValue('name') === 'Grace', 'the debounced text write')
    await until(() => host.stateValue('count') === 4, 'the stepper write')
    await until(() => host.stateValue('color') === 'teal', 'the colour write')
  }}
/>

<!-- Toggle styles, the menu picker (checkmarks, multiple) and the secure field's reveal button. -->
<Story
  name="Toggles, menu picker and secure field"
  play={async ({ canvas, userEvent }) => {
    const notify = await canvas.findByTestId('kit-notify')
    await expect(notify).toHaveAttribute('aria-checked', 'true')
    await userEvent.click(notify)
    await waitFor(() => expect(notify).toHaveAttribute('aria-checked', 'false'))
    await userEvent.click(canvas.getByTestId('kit-bold'))
    await waitFor(() =>
      expect(canvas.getByTestId('kit-bold')).toHaveAttribute('aria-pressed', 'true')
    )
    await userEvent.click(canvas.getByTestId('kit-agree'))
    await waitFor(() =>
      expect(canvas.getByTestId('kit-agree')).toHaveAttribute('aria-checked', 'true')
    )

    await userEvent.click(canvas.getByTestId('kit-tags'))
    const menu = await canvas.findByTestId('kit-tags-menu')
    await waitFor(() => expect(menu).toBeVisible())
    await userEvent.click(canvas.getByTestId('kit-tags-menu-b'))
    await waitFor(() => expect(canvas.getByTestId('kit-tags')).toHaveAccessibleName(/Alpha, Beta/))
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('kit-tags-menu')).toBeNull())
    await expect(canvas.getByTestId('kit-tags')).toHaveFocus()

    const secret = canvas.getByTestId('kit-secret')
    await expect(secret).toHaveAttribute('type', 'password')
    await userEvent.click(canvas.getByTestId('kit-secret-reveal'))
    await waitFor(() => expect(secret).toHaveAttribute('type', 'text'))
  }}
/>

<!-- The display components: text styles, label, icon, image placeholder, badges, avatars, progress. -->
<Story
  name="Display"
  args={{ scenario: 'display' }}
  play={async ({ canvas }) => {
    await expect(await canvas.findByTestId('kit-title')).toHaveAttribute('role', 'heading')
    await expect(canvas.getByTestId('kit-count')).toHaveAccessibleName('99 plus unread')
    await expect(canvas.getByTestId('kit-ada')).toHaveAccessibleName('Ada Lovelace (online)')
    await expect(canvas.getByTestId('kit-cover')).toHaveAccessibleName('Project cover')
    await expect(
      canvas.getByTestId('kit-bar').querySelector('[role="progressbar"]')
    ).toHaveAttribute('aria-valuenow', '35')
    await expect(canvas.getByTestId('kit-icon')).toHaveAccessibleName('Starred')
  }}
/>

<!-- A two-way editor with drafts: typing shows at once; an agent append while typing is held, then merged. -->
<Story
  name="Editor with drafts"
  args={{ scenario: 'editor' }}
  play={async ({ canvas, userEvent }) => {
    const body = await canvas.findByTestId('kit-body')
    await waitFor(() => expect(body).toHaveValue('# Groceries\n\n- milk\n- eggs\n'))
    await userEvent.click(body)
    await userEvent.keyboard('{Control>}{End}{/Control}done')
    // The preview bound to the same ref follows every keystroke, with no round trip.
    await waitFor(() => expect(canvas.getByTestId('kit-preview')).toHaveTextContent('done'))
    await expect(await canvas.findByText('Saving…')).toBeVisible()
    const { host } = await story()
    host.setState('body', `${host.stateValue('body') as string}- butter\n`) // an agent appends while the person types
    await expect(body).toHaveValue('# Groceries\n\n- milk\n- eggs\ndone') // shielded: no caret jump, nothing clobbered
    await until(
      () => String(host.stateValue('body')).includes('done'),
      'the debounced write and the host merge'
    )
    const value = (): string => (body as HTMLTextAreaElement).value
    await waitFor(() => expect(value()).toContain('- butter')) // the agent's line survived
    await expect(value()).toContain('done') // and so did what the person typed
    await waitFor(() => expect(canvas.queryByText('Saving…')).toBeNull())
  }}
/>

<!-- A sheet: focus moves in and stays, the window behind is inert, Escape closes, focus returns to the opener. -->
<Story
  name="Sheet"
  args={{ scenario: 'presentations' }}
  play={async ({ canvas, userEvent }) => {
    const opener = await canvas.findByTestId('kit-open-sheet')
    await userEvent.click(opener)
    const sheet = await canvas.findByTestId('kit-sheet')
    await waitFor(() => expect(sheet).toHaveFocus())
    await expect(canvas.getByTestId('kit-tree')).toHaveAttribute('inert')
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('kit-sheet')).toBeNull())
    await waitFor(() => expect(canvas.getByTestId('kit-tree')).not.toHaveAttribute('inert'))
    await waitFor(() => expect(opener).toHaveFocus())

    await userEvent.click(opener)
    await userEvent.type(await canvas.findByTestId('kit-sheet-name'), 'Recipes')
    await userEvent.click(canvas.getByTestId('kit-sheet-done'))
    const { host } = await story()
    await until(
      () => String(host.stateValue('last')).startsWith('folder-created done'),
      'the host heard the event'
    )
    await waitFor(() => expect(canvas.queryByTestId('kit-sheet')).toBeNull())
  }}
/>

<!-- Alert (cancel on Escape, destructive action), a prompt with a field, a confirmation dialog and a popover. -->
<Story
  name="Alert, prompt, confirmation and popover"
  args={{ scenario: 'presentations' }}
  play={async ({ canvas, userEvent }) => {
    const { host } = await story()
    await userEvent.click(await canvas.findByTestId('kit-open-alert'))
    const alert = await canvas.findByTestId('kit-alert')
    await expect(alert).toHaveAttribute('role', 'alertdialog')
    await waitFor(() => expect(canvas.getByTestId('kit-alert-action-alert-cancel')).toHaveFocus()) // no preferred action: cancel
    await userEvent.keyboard('{Escape}') // runs the cancel action: no event for the app
    await waitFor(() => expect(canvas.queryByTestId('kit-alert')).toBeNull())
    await userEvent.click(canvas.getByTestId('kit-open-alert'))
    await userEvent.click(await canvas.findByTestId('kit-alert-action-alert-delete'))
    await until(
      () => String(host.stateValue('last')).startsWith('deleted alert-delete'),
      'the delete event'
    )

    await userEvent.click(canvas.getByTestId('kit-open-prompt'))
    const field = await canvas.findByTestId('kit-prompt-field-name')
    await waitFor(() => expect(field).toHaveFocus())
    await userEvent.type(field, 'Plans{Enter}')
    await until(
      () => String(host.stateValue('last')) === 'tap prompt-rename {"fields":{"name":"Plans"}}',
      'the prompt values'
    )

    await userEvent.click(canvas.getByTestId('kit-open-confirm'))
    await userEvent.click(await canvas.findByTestId('kit-confirm-action-confirm-discard'))
    await until(
      () => String(host.stateValue('last')).startsWith('discarded confirm-discard'),
      'the confirmation'
    )

    await userEvent.click(canvas.getByTestId('kit-open-popover'))
    await waitFor(async () => expect(await canvas.findByTestId('kit-popover')).toBeVisible())
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('kit-popover')).toBeNull())
  }}
/>

<!-- A menu with a toggle, an inline picker and a nested menu, and a context menu by right-click and by keyboard. -->
<Story
  name="Menu and context menu"
  args={{ scenario: 'menus' }}
  play={async ({ canvas, userEvent }) => {
    const { host } = await story()
    const trigger = await canvas.findByTestId('kit-actions')
    await userEvent.click(trigger)
    await waitFor(async () => expect(await canvas.findByTestId('kit-actions-menu')).toBeVisible())
    await userEvent.click(canvas.getByTestId('kit-actions-menu-pin'))
    await until(() => host.stateValue('pinned') === false, 'the toggle write')
    await userEvent.click(canvas.getByTestId('kit-last')) // a press outside closes it
    await waitFor(() => expect(canvas.queryByTestId('kit-actions-menu')).toBeNull())

    await userEvent.click(trigger)
    await userEvent.click(await canvas.findByTestId('kit-actions-menu-move'))
    await userEvent.click(await canvas.findByTestId('kit-actions-menu-archive'))
    await until(() => String(host.stateValue('last')) === 'archive tap', 'the nested menu item')

    // Context menu: right-click, then the keyboard path.
    const card = canvas.getByTestId('kit-card')
    await fireEvent.contextMenu(card, { clientX: 40, clientY: 40 })
    await userEvent.click(await canvas.findByTestId('kit-card-context-menu-rename'))
    await until(() => String(host.stateValue('last')) === 'rename tap', 'the context menu item')
    card.focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await waitFor(async () =>
      expect(await canvas.findByTestId('kit-card-context-menu')).toBeVisible()
    )
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('kit-card-context-menu')).toBeNull())
    await waitFor(() => expect(card).toHaveFocus())
  }}
/>

<!-- Markdown: structure renders, HTML and bad links stay text, a good link opens through the window's openUrl. -->
<Story
  name="Markdown"
  args={{ scenario: 'markdown' }}
  play={async ({ canvas, userEvent }) => {
    const doc = await canvas.findByTestId('kit-doc')
    await expect(doc.querySelector('h1')).toHaveTextContent('Release notes')
    await expect(doc.querySelector('table')).toBeInTheDocument()
    await expect(doc.querySelector('script')).toBeNull()
    await expect(doc).toHaveTextContent('<scr' + 'ipt>alert(1)</scr' + 'ipt>')
    await expect(doc.querySelectorAll('a')).toHaveLength(1)
    await userEvent.click(canvas.getByTestId('kit-doc-link-0'))
    const { links } = await story()
    await waitFor(() => expect(links).toContain('https://example.com/docs'))
  }}
/>

<!-- Drag and drop: by keyboard (Alt+Arrow), by the "Move to" menu, and by pointer. -->
<Story
  name="Drag and drop"
  args={{ scenario: 'dnd' }}
  play={async ({ canvas, userEvent }) => {
    const { host } = await story()
    const last = await canvas.findByTestId('kit-last')

    const a = canvas.getByTestId('kit-todo-card-a')
    a.focus()
    await userEvent.keyboard('{Alt>}{ArrowRight}{/Alt}')
    await until(
      () => String(host.stateValue('last')) === 'moved a to doing (inside)',
      'the keyboard move'
    )
    await waitFor(() => expect(last).toHaveTextContent('moved a to doing (inside)'))

    const b = canvas.getByTestId('kit-todo-card-b')
    b.focus()
    await userEvent.keyboard('{Shift>}{F10}{/Shift}')
    await userEvent.click(await canvas.findByTestId('kit-todo-card-b-context-menu-move-to'))
    await userEvent.click(await canvas.findByTestId('kit-todo-card-b-context-menu-move-done'))
    await until(
      () => String(host.stateValue('last')) === 'moved b to done (inside)',
      'the Move to menu'
    )

    // The pointer path: press, move past the threshold onto the Doing column, release.
    const doing = canvas.getByTestId('kit-doing-doing-title').getBoundingClientRect()
    const from = a.getBoundingClientRect()
    await fireEvent.pointerDown(a, {
      clientX: from.left + 8,
      clientY: from.top + 8,
      button: 0,
      pointerType: 'mouse',
      pointerId: 1
    })
    await fireEvent.pointerMove(a, {
      clientX: from.left + 30,
      clientY: from.top + 30,
      pointerType: 'mouse',
      pointerId: 1
    })
    await fireEvent.pointerMove(a, {
      clientX: doing.left + doing.width / 2,
      clientY: doing.top + doing.height / 2,
      pointerType: 'mouse',
      pointerId: 1
    })
    await expect(canvas.getByTestId('kit-doing-decorated')).toHaveAttribute('data-over', 'true')
    await fireEvent.pointerUp(a, {
      clientX: doing.left + doing.width / 2,
      clientY: doing.top + doing.height / 2,
      pointerType: 'mouse',
      pointerId: 1
    })
    await until(
      () =>
        String(host.stateValue('last')) === 'moved a to doing (inside)' &&
        host.received.filter((r) => r.method === 'view.event').length >= 3,
      'the pointer drop'
    )
  }}
/>

<!-- A toast sent by the host: kit look, an action button that reports the `toast` event, dismissable. -->
<Story
  name="Toast with action"
  args={{ scenario: 'toast' }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(await canvas.findByTestId('kit-delete'))
    await expect(await canvas.findByText('Note deleted')).toBeVisible()
    const { host } = await story()
    await userEvent.click(await canvas.findByTestId('kit-notice-1-action'))
    await until(() => host.stateValue('last') === 'undo tapped', 'the toast event')
    await waitFor(() => expect(canvas.queryByText('Note deleted')).toBeNull())
  }}
/>
