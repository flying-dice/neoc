<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, userEvent, waitFor, within } from 'storybook/test'
  import SearchField from './SearchField.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/Pickers and fields',
    tags: ['autodocs'],
    parameters: { controls: { disable: true } }
  })
</script>

<script lang="ts">
  let query = $state('')
</script>

<Story name="Search field" asChild>
  <div style="width:300px;display:grid;gap:12px">
    <SearchField testid="search" bind:value={query} />
    <SearchField testid="search-filled" value="swift" />
  </div>
</Story>

<Story
  name="Search field clears"
  asChild
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    const input = canvas.getByTestId('search')
    await userEvent.type(input, 'notes')
    await userEvent.click(canvas.getByTestId('search-clear'))
    await waitFor(() => expect(input).toHaveValue(''))
    await expect(input).toHaveFocus()
    await userEvent.type(input, 'notes')
    await userEvent.keyboard('{Escape}')
    await waitFor(() => expect(input).toHaveValue(''))
    await expect(input).not.toHaveFocus()
  }}
>
  <div style="width:300px"><SearchField testid="search" bind:value={query} /></div>
</Story>
