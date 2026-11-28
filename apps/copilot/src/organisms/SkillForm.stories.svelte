<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SkillForm from './SkillForm.svelte'
  import { skillDraftOf, type SkillDraft } from '../lib/form'
  import { REVIEW } from '../lib/fakeApi'

  interface Args {
    form: SkillDraft
    failure?: string | null
    savable: boolean
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()

  /** The form a skill is added or changed in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/SkillForm',
    component: SkillForm,
    tags: ['autodocs'],
    args: { form: skillDraftOf(null), savable: false, onsave, oncancel, onremove },
    beforeEach: () => {
      onsave.mockClear()
      oncancel.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:560px;color:var(--label);font:var(--type-body)">
    <SkillForm {...args} />
  </div>
{/snippet}

<!-- A new skill: nothing to add until it has a name and its SKILL.md, and nothing to remove. -->
<Story
  name="A new skill"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-skill-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.queryByTestId('copilot-skill-form-remove')).toBeNull()
  }}
/>

<!-- A skill with a file of its own. -->
<Story
  name="A skill"
  args={{ form: skillDraftOf(REVIEW), savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('copilot-skill-form-file-0-path')).toHaveProperty('value', 'scripts/check.sh')
    await userEvent.click(canvas.getByTestId('copilot-skill-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('copilot-skill-form-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- The backend refused the form: why, over the fields. -->
<Story
  name="Refused"
  args={{ form: { ...skillDraftOf(REVIEW), name: 'my skill' }, savable: true, failure: 'A skill is named with letters, digits, ., _ and -, starting with a letter or a digit.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-skill-form-failure')).toHaveTextContent('A skill is named with letters')
  }}
/>
