<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Issue = VosApp.Projects.Issue
  import IssueCard from './IssueCard.svelte'
  import { issue } from '../lib/fakeApi'

  interface Args {
    issue: Issue
    next?: string
    onopen: () => void
    onadvance: () => void
    ondrag: (dragging: boolean) => void
  }

  const onopen = fn<() => void>()
  const onadvance = fn<() => void>()
  const ondrag = fn<(dragging: boolean) => void>()

  /** One issue on the board. It keeps nothing: a tap and the wish to move it go out. */
  const { Story } = defineMeta({
    title: 'Molecules/IssueCard',
    component: IssueCard,
    tags: ['autodocs'],
    args: { issue: issue(1, 'Write the home page'), next: 'In Progress', onopen, onadvance, ondrag },
    beforeEach: () => {
      onopen.mockClear()
      onadvance.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:260px;margin:0;padding:12px;background:var(--bg-grouped);color:var(--label)">
    <IssueCard {...args} />
  </ul>
{/snippet}

<Story
  name="A task"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('projects-card-WEB-1-open'))
    await expect(onopen).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('projects-card-WEB-1-advance'))
    await expect(onadvance).toHaveBeenCalledTimes(1)
    await expect(canvas.getByTestId('projects-card-WEB-1-advance')).toHaveProperty('label', 'Move WEB-1 to In Progress')
  }}
/>

<!-- A pressing bug says so. -->
<Story name="A pressing bug" args={{ issue: issue(2, 'Fix the footer links', { type: 'bug', priority: 'high' }) }} />

<!-- In the last column there is nowhere further to move it. -->
<Story
  name="Done"
  args={{ issue: issue(5, 'Register the domain', { type: 'story', priority: 'low', status: 'done' }), next: undefined }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('projects-card-WEB-5-advance')).toBeNull()
  }}
/>
