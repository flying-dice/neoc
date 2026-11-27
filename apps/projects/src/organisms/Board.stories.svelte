<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Board from './Board.svelte'
  type Issue = VosApp.Projects.Issue
  type Status = VosApp.Projects.Status
  import { column } from '../lib/catalog'
  import { ISSUES } from '../lib/samples'

  interface Args {
    columns: Readonly<Record<Status, readonly Issue[]>>
    draft: string
    onopen: (issue: Issue) => void
    onmove: (issue: Issue, status: Status) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  const web = ISSUES.filter((i) => i.project === 1)
  const columns = { todo: column(web, 'todo'), doing: column(web, 'doing'), done: column(web, 'done') }
  const onopen = fn<(issue: Issue) => void>()
  const onmove = fn<(issue: Issue, status: Status) => void>()
  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** A project's board. It keeps nothing but the card that is being dragged: the moves go out. */
  const { Story } = defineMeta({
    title: 'Organisms/Board',
    component: Board,
    tags: ['autodocs'],
    args: { columns, draft: '', onopen, onmove, ondraft, onadd },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:760px;height:460px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <Board {...args} />
  </div>
{/snippet}

<Story
  name="With issues"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-column-todo-count')).toHaveTextContent('3')
    // the most pressing is first
    await expect(canvas.getAllByTestId(/^projects-card-WEB-\d+$/)[0]).toHaveAttribute('data-testid', 'projects-card-WEB-2')
    await userEvent.click(canvas.getByTestId('projects-card-WEB-3-advance'))
    await expect(onmove).toHaveBeenCalledWith(web[2], 'done')
  }}
/>

<Story name="Empty" args={{ columns: { todo: [], doing: [], done: [] } }} />
