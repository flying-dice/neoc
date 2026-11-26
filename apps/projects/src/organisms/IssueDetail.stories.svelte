<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, textArea } from '@vos/storybook/kit'
  import IssueDetail from './IssueDetail.svelte'
  type Issue = VosApp.Projects.Issue
  import { ISSUES } from '../lib/samples'
  import type { IssueChanges } from '../lib/projects.svelte'

  interface Args {
    issue: Issue
    from: string
    onchange: (issue: Issue, changes: IssueChanges) => void
    onremove: (issue: Issue) => void
    onback: () => void
  }

  const bug = ISSUES[1]!
  const onchange = fn<(issue: Issue, changes: IssueChanges) => void>()
  const onremove = fn<(issue: Issue) => void>()
  const onback = fn<() => void>()

  /** One issue, open. It keeps nothing: each change goes out as it is made. */
  const { Story } = defineMeta({
    title: 'Organisms/IssueDetail',
    component: IssueDetail,
    tags: ['autodocs'],
    args: { issue: bug, from: 'Board', onchange, onremove, onback },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:560px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <IssueDetail {...args} />
  </div>
{/snippet}

<Story
  name="A bug"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-issue-key')).toHaveTextContent('WEB-2')
    await expect(textArea(canvas.getByTestId('projects-issue-description'))).toHaveValue('They go nowhere on the pricing page.')
    await userEvent.click(canvas.getByTestId('projects-issue-status').shadowRoot!.querySelector('[data-testid="projects-issue-status-doing"]')!)
    await expect(onchange).toHaveBeenCalledWith(bug, { status: 'doing' })
  }}
/>

<Story
  name="Deleting asks first"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('projects-issue-remove'))
    await expect(onremove).not.toHaveBeenCalled()
    await userEvent.click(alertButton(canvas.getByTestId('projects-issue-confirm'), 'action'))
    await expect(onremove).toHaveBeenCalledWith(bug)
  }}
/>
