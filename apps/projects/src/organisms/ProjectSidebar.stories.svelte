<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import ProjectSidebar from './ProjectSidebar.svelte'
  import type { Project } from '@projects/rpc'
  import { WEBSITE } from '../lib/fakeApi'
  import { APP } from '../lib/samples'

  interface Args {
    projects: readonly Project[]
    open: ReadonlyMap<number, number>
    selected?: number
    onselect: (project: Project) => void
    onnew: () => void
  }

  const onselect = fn<(project: Project) => void>()
  const onnew = fn<() => void>()

  /** The projects, and the way to start one. It keeps nothing: the pick and the wish for a new one go out. */
  const { Story } = defineMeta({
    title: 'Organisms/ProjectSidebar',
    component: ProjectSidebar,
    tags: ['autodocs'],
    args: { projects: [WEBSITE, APP], open: new Map([[1, 4], [2, 1]]), selected: 1, onselect, onnew },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:280px;height:420px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <ProjectSidebar {...args} />
  </div>
{/snippet}

<Story
  name="Projects"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-project-WEB')).toHaveProperty('selected', true)
    await expect(canvas.getByTestId('projects-project-WEB')).toHaveProperty('label', 'Website, WEB, 4 open')
    await userEvent.click(canvas.getByTestId('projects-project-APP'))
    await expect(onselect).toHaveBeenCalledWith(APP)
    await userEvent.click(canvas.getByTestId('projects-new'))
    await expect(onnew).toHaveBeenCalled()
  }}
/>

<Story
  name="None yet"
  args={{ projects: [], open: new Map(), selected: undefined }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-list-empty')).toBeVisible()
  }}
/>
