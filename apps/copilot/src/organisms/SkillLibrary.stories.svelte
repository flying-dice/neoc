<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SkillLibrary from './SkillLibrary.svelte'
  import { REVIEW } from '../lib/fakeApi'

  type Skill = VosApp.Copilot.Skill

  interface Args {
    skills: readonly Skill[]
    back?: boolean
    onedit: (skill: Skill | null) => void
    onback?: () => void
  }

  const onedit = fn<(skill: Skill | null) => void>()
  const onback = fn<() => void>()
  const DEPLOY: Skill = { id: 'deploy', name: 'deploy', text: '# Deploy', files: [] }

  /** The skill library. */
  const { Story } = defineMeta({
    title: 'Organisms/SkillLibrary',
    component: SkillLibrary,
    tags: ['autodocs'],
    args: { skills: [DEPLOY, REVIEW], onedit, onback },
    beforeEach: () => {
      onedit.mockClear()
      onback.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:420px;color:var(--label);font:var(--type-body)">
    <SkillLibrary {...args} />
  </div>
{/snippet}

<!-- Each skill by its name and its files; a tap opens it, and a skill is added from the last row. -->
<Story
  name="Skills"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('copilot-skill-review')).toHaveProperty('subheading', 'SKILL.md and 1 file')
    await expect(canvas.getByTestId('copilot-skill-deploy')).toHaveProperty('subheading', 'SKILL.md')
    await userEvent.click(canvas.getByTestId('copilot-skill-review'))
    await expect(onedit).toHaveBeenCalledWith(REVIEW)
    await userEvent.click(canvas.getByTestId('copilot-add-skill'))
    await expect(onedit).toHaveBeenLastCalledWith(null)
  }}
/>

<!-- An empty library. -->
<Story name="No skills yet" args={{ skills: [] }} />

<!-- A window too narrow for the list as well: the way back to it. -->
<Story
  name="With the way back"
  args={{ back: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('copilot-skills-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
