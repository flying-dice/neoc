<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { ComponentProps } from 'svelte'
  import type { Note } from 'app:rpc'
  import NotePane from './NotePane.svelte'
  import type { Mode } from '../lib/notebooks.svelte'

  type Args = ComponentProps<typeof NotePane>

  const BODY = 'Basil, thyme and chives by the kitchen door. Part of the [[Garden plan]], next to the [[Cold frame]].\n\n- [x] Basil\n- [ ] Thyme\n\n#garden #kitchen'
  const NOTE: Note = { id: 3, notebook: 1, title: 'Herbs', body: BODY, pinned: false, updated: '2026-06-09T08:00:00.000Z', backlinks: [] }
  const BACKLINKS = [
    { id: 1, title: 'Garden plan', excerpt: 'See Tomatoes and the herb bed.' },
    { id: 2, title: 'Tomatoes', excerpt: 'They share a bed with basil: see Herbs.' }
  ]

  const mocks = {
    onedit: fn<(text: string) => void>(),
    onflush: fn<() => void>(),
    onmode: fn<(mode: Mode) => void>(),
    onretitle: fn<(title: string) => void>(),
    onrename: fn<(title: string) => void>(),
    onpin: fn<() => void>(),
    onremove: fn<() => void>(),
    onfollow: fn<(title: string) => void>(),
    onlink: fn<(url: string) => void>(),
    ontag: fn<(tag: string) => void>(),
    ontick: fn<(line: number) => void>(),
    onopen: fn<(id: number) => void>(),
    onback: fn<() => void>()
  }

  /** The kit's text field holds the real input in its shadow tree. */
  const title = (canvas: { getByTestId(id: string): HTMLElement }): HTMLInputElement =>
    canvas.getByTestId('notebooks-title').shadowRoot!.querySelector('input')!

  /** One note. It keeps only whether deleting is being asked about: what is typed and pressed goes out. */
  const { Story } = defineMeta({
    title: 'Organisms/NotePane',
    component: NotePane,
    tags: ['autodocs'],
    args: {
      note: NOTE,
      body: BODY,
      mode: 'edit',
      status: 'saved',
      backlinks: [],
      resolves: (name) => name !== 'Cold frame',
      ...mocks
    },
    argTypes: { resolves: { control: false } },
    beforeEach: () => {
      for (const mock of Object.values(mocks)) mock.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <NotePane {...args} />
  </div>
{/snippet}

<!-- The note as it is written: Markdown, in a field that fills the pane. -->
<Story
  name="Writing"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-editor').shadowRoot!.querySelector('textarea')!).toHaveValue(BODY)
    await expect(canvas.getByTestId('notebooks-status')).toHaveTextContent('Saved')
    await expect(title(canvas)).toHaveValue('Herbs')
  }}
/>

<!-- The note as it reads, with the notes that link to it under it. -->
<Story
  name="Reading, with what links here"
  args={{ mode: 'read', backlinks: BACKLINKS, note: { ...NOTE, pinned: true } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-reading-link-Garden plan')).toBeVisible()
    await expect(canvas.getByTestId('notebooks-backlink-2')).toHaveTextContent('Tomatoes')
    await expect(canvas.getByTestId('notebooks-pin')).toHaveAttribute('pressed')
  }}
/>

<Story name="A new note" args={{ note: { ...NOTE, title: 'Untitled', body: '' }, body: '' }} />

<Story
  name="Nothing to read"
  args={{ note: { ...NOTE, body: '' }, body: '', mode: 'read' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-blank')).toBeVisible()
  }}
/>

<Story name="Not yet saved" args={{ status: 'edited' }} />

<!-- A title the note cannot have: the field is marked, and the reason is under it. -->
<Story
  name="A title that is taken"
  args={{ titleProblem: 'There is already a note called Tomatoes.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-title-problem')).toHaveTextContent('There is already a note called Tomatoes.')
    await expect(canvas.getByTestId('notebooks-title')).toHaveAttribute('invalid')
  }}
/>

<Story
  name="In a narrow window"
  args={{ back: true, mode: 'read', backlinks: BACKLINKS, note: { ...NOTE, title: 'A title long enough that it has to be cut short' } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('notebooks-back')).toBeVisible()
  }}
/>

<Story
  name="Writing and pressing"
  tags={['play-only']}
  args={{ backlinks: BACKLINKS }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.type(canvas.getByTestId('notebooks-editor').shadowRoot!.querySelector('textarea')!, '!')
    await expect(mocks.onedit).toHaveBeenCalledWith(`${BODY}!`)
    await userEvent.type(title(canvas), ' bed{Enter}')
    await expect(mocks.onretitle).toHaveBeenLastCalledWith('Herbs bed')
    await expect(mocks.onflush).toHaveBeenCalled() // the writing was left for the title
    await expect(mocks.onrename).toHaveBeenCalledWith('Herbs bed')
    await userEvent.click(canvas.getByTestId('notebooks-mode').shadowRoot!.querySelector('[data-testid="notebooks-mode-read"]')!)
    await expect(mocks.onmode).toHaveBeenCalledWith('read')
    await userEvent.click(canvas.getByTestId('notebooks-pin'))
    await expect(mocks.onpin).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('notebooks-backlink-1'))
    await expect(mocks.onopen).toHaveBeenCalledWith(1)
    await userEvent.click(canvas.getByTestId('notebooks-delete'))
    await expect(canvas.getByTestId('notebooks-confirm').shadowRoot!.querySelector('dialog')).toHaveTextContent('Delete Herbs?')
    await userEvent.click(canvas.getByTestId('notebooks-confirm').shadowRoot!.querySelector<HTMLElement>('[part=action]')!)
    await expect(mocks.onremove).toHaveBeenCalledTimes(1)
  }}
/>
