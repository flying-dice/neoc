<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent } from 'storybook/test'
  import StatusBar from './StatusBar.svelte'

  const { Story } = defineMeta({
    title: 'Organisms/StatusBar',
    component: StatusBar,
    tags: ['autodocs'],
    parameters: { layout: 'padded', controls: { disable: true } }
  })

  const searched = fn()
</script>

{#snippet bar(search: boolean, bare = false)}
  <div
    data-testid="bar"
    style="width:640px;max-width:100%;padding:6px 0 60px;background:linear-gradient(135deg,#6e8efb,#c471ed);border-radius:var(--radius-xl)"
  >
    <StatusBar testid="status-bar" {bare} onsearch={search ? searched : undefined} searchShortcut="Meta+K">
      {#snippet center()}<span data-testid="center">Reconnecting…</span>{/snippet}
    </StatusBar>
  </div>
{/snippet}

<!-- The bar as it is without system search: the time and the date. -->
<Story
  name="Plain"
  asChild
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('status-bar-search')).toBeNull()
  }}
>
  {@render bar(false)}
</Story>

<!--
  With system search: a magnifier at the trailing edge, a button named "Search" that says it opens a dialog and which
  shortcut does the same. Its target is bigger than its glyph. Enter and a click both ask for search.
-->
<Story
  name="Search button"
  asChild
  play={async ({ canvas }) => {
    searched.mockClear()
    const button = await canvas.findByTestId('status-bar-search')
    await expect(button).toHaveAccessibleName('Search')
    await expect(button).toHaveAttribute('aria-haspopup', 'dialog')
    await expect(button).toHaveAttribute('aria-keyshortcuts', 'Meta+K')
    await userEvent.click(button)
    await expect(searched).toHaveBeenCalledTimes(1)
    button.focus()
    await userEvent.keyboard('{Enter}')
    await expect(searched).toHaveBeenCalledTimes(2)
    // the pressable area reaches past the 24 px glyph
    const target = getComputedStyle(button, '::before')
    await expect(button.offsetWidth - parseFloat(target.left) - parseFloat(target.right)).toBeGreaterThanOrEqual(44)
    await expect(button.offsetHeight - parseFloat(target.top) - parseFloat(target.bottom)).toBeGreaterThanOrEqual(44)
  }}
>
  {@render bar(true)}
</Story>

<!-- Turned off in Settings: no time, date or search, and what the shell must say (the centre) still shows. -->
<Story
  name="Bare"
  asChild
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('status-bar-search')).toBeNull()
    await expect(canvas.getByTestId('bar')).toHaveTextContent(/^\s*Reconnecting…\s*$/)
  }}
>
  {@render bar(true, true)}
</Story>
