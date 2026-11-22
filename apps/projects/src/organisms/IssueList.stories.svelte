<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import IssueList from './IssueList.svelte'
  import type { Issue } from 'app:rpc'
  import { ISSUES } from '../lib/samples'

  interface Args {
    issues: readonly Issue[]
    draft: string
    onopen: (issue: Issue) => void
    ondraft: (text: string) => void
    onadd: () => void
  }

  const web = ISSUES.filter((i) => i.project === 1)
  const onopen = fn<(issue: Issue) => void>()
  const ondraft = fn<(text: string) => void>()
  const onadd = fn<() => void>()

  /** Every issue of a project, in the order they were added. */
  const { Story } = defineMeta({
    title: 'Organisms/IssueList',
    component: IssueList,
    tags: ['autodocs'],
    args: { issues: web, draft: '', onopen, ondraft, onadd },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:640px;height:460px;background:var(--bg-grouped);color:var(--label);font:var(--type-body)">
    <IssueList {...args} />
  </div>
{/snippet}

<Story
  name="With issues"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('projects-issue-row-WEB-2')).toHaveProperty('subheading', 'WEB-2 · Bug · High')
    await userEvent.click(canvas.getByTestId('projects-issue-row-WEB-2'))
    await expect(onopen).toHaveBeenCalledWith(web[1])
  }}
/>

<Story
  name="Empty"
  args={{ issues: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('projects-issues-empty')).toBeVisible()
  }}
/>
