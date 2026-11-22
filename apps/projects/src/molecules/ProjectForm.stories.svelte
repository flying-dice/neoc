<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ProjectForm from './ProjectForm.svelte'
  import type { Problems, ProjectDraft } from '../lib/projects.svelte'

  interface Args {
    draft: ProjectDraft
    problems: Problems<'name' | 'key'>
    ready: boolean
    refused: string | null
    busy: boolean
    ondraft: (change: Partial<ProjectDraft>) => void
    onadd: () => void
    oncancel: () => void
  }

  const ondraft = fn<(change: Partial<ProjectDraft>) => void>()
  const onadd = fn<() => void>()
  const oncancel = fn<() => void>()
  const NOT_A_KEY = 'A key is two to five letters or digits, a letter first, like WEB.'

  /** A new project as it is typed. Add cannot be pressed until there is a name and a key with nothing wrong with them. */
  const { Story } = defineMeta({
    title: 'Molecules/ProjectForm',
    component: ProjectForm,
    tags: ['autodocs'],
    args: { draft: { name: '', key: '' }, problems: { name: null, key: null }, ready: false, refused: null, busy: false, ondraft, onadd, oncancel },
    beforeEach: () => {
      ondraft.mockClear()
      onadd.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;padding:16px 0;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <ProjectForm {...args} />
  </div>
{/snippet}

<!-- Nothing to add yet: Add cannot be pressed, and the footer says what the key is for. -->
<Story
  name="Empty"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-form-add')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footerTone', 'note')
    await userEvent.click(canvas.getByTestId('projects-form-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="Typed"
  args={{ draft: { name: 'Website', key: 'WEB' }, ready: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footer', 'Its issues are numbered under the key: WEB-1, WEB-2.')
    await userEvent.click(canvas.getByTestId('projects-form-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A key that is wrong is quiet until the person leaves it: then the footer says what is wrong, and Add cannot be pressed. -->
<Story
  name="A key that is not a key"
  args={{ draft: { name: 'Website', key: '1' }, problems: { name: null, key: NOT_A_KEY } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footerTone', 'note')
    await userEvent.click(canvas.getByTestId('projects-form-key').shadowRoot!.querySelector('input')!)
    await userEvent.tab()
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footer', NOT_A_KEY)
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footerTone', 'error')
    await expect(canvas.getByTestId('projects-form-key')).toHaveProperty('invalid', true)
    await expect(canvas.getByTestId('projects-form-add')).toHaveProperty('disabled', true)
  }}
/>

<!-- What only the backend knows comes back to the same footer. -->
<Story
  name="A key that is taken"
  args={{ draft: { name: 'Web Shop', key: 'WEB' }, ready: true, refused: 'Another project has the key WEB.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footer', 'Another project has the key WEB.')
    await expect(canvas.getByTestId('projects-form')).toHaveProperty('footerTone', 'error')
    await expect(canvas.getByTestId('projects-form-key')).toHaveProperty('invalid', true)
  }}
/>

<Story name="Adding" args={{ draft: { name: 'Website', key: 'WEB' }, ready: true, busy: true }} />
