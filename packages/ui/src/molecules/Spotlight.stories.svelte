<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor } from 'storybook/test'
  import Spotlight from './Spotlight.svelte'
  import SearchPill from './SearchPill.svelte'
  import type { SpotlightItem } from '../types'

  const { Story } = defineMeta({
    title: 'Molecules/Spotlight',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })

  const everything: SpotlightItem[] = [
    { id: 'app-calculator', title: 'Calculator', kind: 'App', icon: 'calculator', background: 'linear-gradient(180deg, #ffa733, #ff9500)' },
    { id: 'app-files', title: 'Files', kind: 'App', icon: 'folder', background: 'linear-gradient(180deg, #5ac8fa, #007aff)' },
    { id: 'app-notes', title: 'Notes', kind: 'App', icon: 'doc', background: 'linear-gradient(180deg, #ffe066, #ffcc00)' },
    { id: 'settings-appearance', title: 'Appearance', kind: 'Settings', icon: 'sun', background: 'var(--ios-blue)' },
    { id: 'action-home', title: 'Show Home Screen', kind: 'Action', icon: 'house', background: 'var(--ios-gray)' }
  ]

  const activated = fn()
  const dismissed = fn()

  type Canvas = {
    findByTestId: (id: string) => Promise<HTMLElement>
    getByTestId: (id: string) => HTMLElement
    queryByTestId: (id: string) => HTMLElement | null
  }
  const field = (canvas: Canvas): Promise<HTMLElement> => canvas.findByTestId('spotlight-field')
  const focused = async (canvas: Canvas): Promise<HTMLElement> => {
    const input = await field(canvas)
    await waitFor(() => expect(input).toHaveFocus())
    return input
  }
  const option = (canvas: Canvas, id: string): HTMLElement => canvas.getByTestId(`spotlight-result-${id}`)
</script>

<script lang="ts">
  // The owner's part, as small as it can be: what was typed filters the list by title; the highlight is an id.
  let open = $state(true)
  let query = $state('')
  let chosen = $state<string | null>(null)
  const items = $derived(
    query.trim()
      ? everything.filter((i) => i.title.toLowerCase().includes(query.trim().toLowerCase()))
      : everything.slice(0, 3)
  )
  const activeId = $derived(items.some((i) => i.id === chosen) ? chosen : (items[0]?.id ?? null))
  const status = $derived(
    query.trim() ? (items.length ? `${items.length} results` : 'No results') : `${items.length} suggestions`
  )

  function move(delta: 1 | -1): void {
    const at = items.findIndex((i) => i.id === activeId)
    chosen = items[Math.min(Math.max(at + delta, 0), items.length - 1)]?.id ?? null
  }
  function close(): void {
    open = false
    query = ''
    chosen = null
  }
</script>

{#snippet screen(initial = '')}
  <div class="screen" data-testid="screen">
    <div class="behind">
      <SearchPill
        testid="open"
        onclick={() => {
          query = initial
          open = true
        }}
      />
    </div>
    {#if open}
      <Spotlight
        testid="spotlight"
        bind:query
        {items}
        {activeId}
        heading={query.trim() ? undefined : 'Suggestions'}
        {status}
        onmove={move}
        onhighlight={(id) => (chosen = id)}
        onactivate={(id) => {
          activated(id)
          close()
        }}
        ondismiss={() => {
          dismissed()
          close()
        }}
      />
    {/if}
  </div>
{/snippet}

<!--
  As it opens: the field has focus, nothing is typed, and the list under it is what the owner suggests, the first
  highlighted. A dialog holding a combobox that controls a listbox; how many there are is said politely.
-->
<Story
  name="Suggestions"
  asChild
  play={async ({ canvas }) => {
    const input = await focused(canvas)
    const dialog = canvas.getByTestId('spotlight')
    await expect(dialog).toHaveAttribute('role', 'dialog')
    await expect(dialog).toHaveAttribute('aria-modal', 'true')
    await expect(dialog).toHaveAccessibleName('Search')
    await expect(input).toHaveAttribute('role', 'combobox')
    await expect(input).toHaveAttribute('aria-expanded', 'true')
    await expect(input).toHaveAttribute('aria-controls', 'spotlight-results')
    const list = canvas.getByTestId('spotlight-results')
    await expect(list).toHaveAttribute('role', 'listbox')
    await expect(list).toHaveAttribute('id', 'spotlight-results')
    await expect(list).toHaveAccessibleName('Suggestions')
    await expect(list.querySelectorAll('[role="option"]')).toHaveLength(3)
    // the first is highlighted: it is the field's active descendant, and the only selected option
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-calculator')
    await expect(option(canvas, 'app-calculator')).toHaveAttribute('aria-selected', 'true')
    await expect(option(canvas, 'app-files')).toHaveAttribute('aria-selected', 'false')
    const status = canvas.getByTestId('spotlight-status')
    await expect(status).toHaveAttribute('aria-live', 'polite')
    await waitFor(() => expect(status).toHaveTextContent('3 suggestions'))
  }}
>
  {@render screen()}
</Story>

<!-- Typing filters as it goes: the list, the count that is said, and the highlight (back on the top result). -->
<Story
  name="Results"
  asChild
  play={async ({ canvas }) => {
    const input = await focused(canvas)
    await userEvent.type(input, 'e')
    await waitFor(() => expect(canvas.getByTestId('spotlight-results').querySelectorAll('[role="option"]')).toHaveLength(4))
    await expect(canvas.getByTestId('spotlight-status')).toHaveTextContent('4 results')
    await expect(canvas.getByTestId('spotlight-results')).toHaveAccessibleName('Results')
    await expect(option(canvas, 'settings-appearance')).toHaveTextContent('Appearance')
    await expect(option(canvas, 'settings-appearance')).toHaveTextContent('Settings') // what kind of thing it is
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-files')
    await expect(canvas.getByTestId('spotlight-clear')).toHaveAccessibleName('Clear')
  }}
>
  {@render screen()}
</Story>

<!-- Nothing answers what was typed: the panel says so, to the eye and to a screen reader, and the field stays a collapsed combobox. -->
<Story
  name="No results"
  asChild
  play={async ({ canvas }) => {
    const input = await focused(canvas)
    await userEvent.type(input, 'zzzz')
    await expect(await canvas.findByTestId('spotlight-empty')).toHaveTextContent('No Results')
    await expect(canvas.getByTestId('spotlight-status')).toHaveTextContent('No results')
    await expect(input).toHaveAttribute('aria-expanded', 'false')
    await expect(input).not.toHaveAttribute('aria-activedescendant')
    await expect(canvas.getByTestId('spotlight-results').querySelectorAll('[role="option"]')).toHaveLength(0)
    // Enter opens nothing
    activated.mockClear()
    await userEvent.keyboard('{Enter}')
    await expect(activated).not.toHaveBeenCalled()
    await expect(canvas.getByTestId('spotlight')).toBeInTheDocument()
  }}
>
  {@render screen()}
</Story>

<!--
  The keys: Down and Up move the highlight (focus never leaves the field) and stop at the ends; Enter opens the
  highlighted result; the panel goes and focus is back where it was before it opened.
-->
<Story
  name="Keyboard"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    activated.mockClear()
    // open it from the page, so there is somewhere for focus to go back to
    await focused(canvas)
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())
    const opener = canvas.getByTestId('open')
    opener.focus()
    await userEvent.keyboard('{Enter}')
    const input = await focused(canvas)

    await userEvent.keyboard('{ArrowUp}') // the top: stays
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-calculator')
    await userEvent.keyboard('{ArrowDown}')
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-files')
    await expect(option(canvas, 'app-files')).toHaveAttribute('aria-selected', 'true')
    await expect(option(canvas, 'app-calculator')).toHaveAttribute('aria-selected', 'false')
    await expect(input).toHaveFocus() // the results are never focused
    await userEvent.keyboard('{ArrowDown}{ArrowDown}{ArrowDown}') // the end: stays
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-notes')
    await userEvent.keyboard('{ArrowUp}')
    await expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-files')

    // Tab stays inside the panel
    await userEvent.tab()
    await expect(canvas.getByTestId('spotlight').contains(document.activeElement)).toBe(true)
    input.focus()

    await userEvent.keyboard('{Enter}')
    await expect(activated).toHaveBeenCalledWith('app-files')
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())
    await waitFor(() => expect(opener).toHaveFocus())
  }}
>
  {@render screen()}
</Story>

<!-- Escape first clears what was typed (the panel stays, with its suggestions); Escape again closes it, and focus is back where it was. -->
<Story
  name="Escape twice"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    dismissed.mockClear()
    await focused(canvas)
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())
    const opener = canvas.getByTestId('open')
    opener.focus()
    await userEvent.keyboard('{Enter}')
    const input = await focused(canvas)
    dismissed.mockClear()

    await userEvent.type(input, 'calc')
    await expect(input).toHaveValue('calc')
    await userEvent.keyboard('{Escape}')
    await expect(input).toHaveValue('')
    await expect(dismissed).not.toHaveBeenCalled()
    await expect(canvas.getByTestId('spotlight')).toBeInTheDocument()
    await expect(canvas.getByTestId('spotlight-results')).toHaveAccessibleName('Suggestions')
    await expect(input).toHaveFocus()

    await userEvent.keyboard('{Escape}')
    await expect(dismissed).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())
    await waitFor(() => expect(opener).toHaveFocus())
  }}
>
  {@render screen()}
</Story>

<!-- The pointer: resting on a result highlights it, a click opens it; a press on the dimmed screen closes the panel; the clear button empties the field and keeps it focused. -->
<Story
  name="Pointer"
  asChild
  tags={['play-only']}
  play={async ({ canvas }) => {
    activated.mockClear()
    dismissed.mockClear()
    const input = await focused(canvas)
    await userEvent.type(input, 'e')
    await userEvent.click(await canvas.findByTestId('spotlight-clear'))
    await expect(input).toHaveValue('')
    await expect(input).toHaveFocus()

    await userEvent.hover(option(canvas, 'app-notes'))
    await waitFor(() => expect(input).toHaveAttribute('aria-activedescendant', 'spotlight-result-app-notes'))
    await userEvent.click(option(canvas, 'app-notes'))
    await expect(activated).toHaveBeenCalledWith('app-notes')
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())

    await userEvent.click(canvas.getByTestId('open'))
    await focused(canvas)
    await userEvent.pointer({ keys: '[MouseLeft]', target: canvas.getByTestId('spotlight-scrim') })
    await expect(dismissed).toHaveBeenCalledTimes(1)
    await waitFor(() => expect(canvas.queryByTestId('spotlight')).toBeNull())
  }}
>
  {@render screen()}
</Story>

<!-- A key pressed in the panel is the panel's: nothing behind it hears it. -->
<Story
  name="Keys stay in the panel"
  asChild
  tags={['play-only']}
  play={async ({ canvas, canvasElement }) => {
    const heard: string[] = []
    const listen = (e: KeyboardEvent): void => void heard.push(e.key)
    canvasElement.ownerDocument.defaultView!.addEventListener('keydown', listen)
    try {
      const input = await focused(canvas)
      await userEvent.type(input, 'a')
      await userEvent.keyboard('{ArrowDown}{Enter}')
      await expect(heard).toEqual([])
    } finally {
      canvasElement.ownerDocument.defaultView!.removeEventListener('keydown', listen)
    }
  }}
>
  {@render screen()}
</Story>

<style>
  .screen {
    position: relative;
    width: 760px;
    max-width: 100%;
    height: 520px;
    overflow: hidden;
    border-radius: var(--radius-xl);
    background:
      radial-gradient(120% 90% at 10% 0%, #6e8efb 0%, transparent 55%),
      radial-gradient(90% 80% at 100% 10%, #c471ed 0%, transparent 55%),
      radial-gradient(100% 90% at 80% 100%, #f7797d 0%, transparent 55%),
      radial-gradient(90% 90% at 0% 100%, #12c2e9 0%, transparent 60%), #2a2d6b;
  }
  .behind {
    position: absolute;
    left: 0;
    right: 0;
    bottom: 24px;
    display: flex;
    justify-content: center;
  }
</style>
