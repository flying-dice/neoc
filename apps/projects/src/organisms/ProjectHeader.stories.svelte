<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ProjectHeader from './ProjectHeader.svelte'
  import type { Project } from 'app:rpc'
  import { WEBSITE } from '../lib/fakeApi'
  import type { Tab } from '../lib/catalog'

  interface Args {
    project: Project
    tab: Tab
    back?: boolean
    ontab: (tab: Tab) => void
    onremove: (project: Project) => void
    onback?: () => void
  }

  const ontab = fn<(tab: Tab) => void>()
  const onremove = fn<(project: Project) => void>()
  const onback = fn<() => void>()

  /** The project that is shown, and what of it. Deleting it asks first. */
  const { Story } = defineMeta({
    title: 'Organisms/ProjectHeader',
    component: ProjectHeader,
    tags: ['autodocs'],
    args: { project: WEBSITE, tab: 'board', back: false, ontab, onremove, onback },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:140px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <ProjectHeader {...args} />
  </div>
{/snippet}

<Story
  name="On the board"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-title')).toHaveTextContent('Website')
    await userEvent.click(canvas.getByTestId('projects-tab').shadowRoot!.querySelector('[data-testid="projects-tab-pages"]')!)
    await expect(ontab).toHaveBeenCalledWith('pages')
  }}
/>

<Story name="In a narrow window" args={{ back: true, tab: 'pages' }} />

<Story
  name="Deleting asks first"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('projects-delete'))
    await expect(canvas.getByTestId('projects-confirm').shadowRoot!.querySelector('dialog')).toHaveTextContent('Its issues and its pages are deleted with it.')
    await expect(onremove).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('projects-confirm').shadowRoot!.querySelector<HTMLElement>('[part=action]')!)
    await expect(onremove).toHaveBeenCalledWith(WEBSITE)
  }}
/>
