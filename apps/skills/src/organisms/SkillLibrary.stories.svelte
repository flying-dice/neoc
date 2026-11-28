<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SkillLibrary from './SkillLibrary.svelte'
  import { DEPLOY, REVIEW } from '../lib/fakeApi'

  type Skill = VosApp.Skills.Skill

  interface Args {
    skills: readonly Skill[]
    onedit: (skill: Skill | null) => void
  }

  const onedit = fn<(skill: Skill | null) => void>()

  /** The skills. */
  const { Story } = defineMeta({
    title: 'Organisms/SkillLibrary',
    component: SkillLibrary,
    tags: ['autodocs'],
    args: { skills: [DEPLOY, REVIEW], onedit },
    beforeEach: () => {
      onedit.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:420px;color:var(--label);font:var(--type-body)">
    <SkillLibrary {...args} />
  </div>
{/snippet}

<!-- Each skill by its name and what it is for, or its files when its SKILL.md does not say; a tap opens it, and a skill is added from the last row. -->
<Story
  name="Skills"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('skills-skill-review')).toHaveProperty('subheading', 'Review a pull request before it is merged.')
    await expect(canvas.getByTestId('skills-skill-deploy')).toHaveProperty('subheading', 'SKILL.md')
    await userEvent.click(canvas.getByTestId('skills-skill-review'))
    await expect(onedit).toHaveBeenCalledWith(REVIEW)
    await userEvent.click(canvas.getByTestId('skills-add'))
    await expect(onedit).toHaveBeenLastCalledWith(null)
  }}
/>

<!-- No skills yet. -->
<Story name="No skills yet" args={{ skills: [] }} />
