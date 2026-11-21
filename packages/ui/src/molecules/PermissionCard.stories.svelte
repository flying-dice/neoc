<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import PermissionCard from './PermissionCard.svelte'

  const { Story } = defineMeta({
    title: 'Molecules/PermissionCard',
    component: PermissionCard,
    tags: ['autodocs'],
    parameters: { layout: 'padded', controls: { disable: true } }
  })

  const onchoose = fn()
  const options = [
    { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
    { optionId: 'always', name: 'Always allow', kind: 'allow_always' },
    { optionId: 'deny', name: 'Deny', kind: 'reject_once' }
  ]
</script>

<Story name="Asking" asChild>
  <PermissionCard testid="permission" title="Calculator · calculate" {options} {onchoose} />
</Story>

<Story name="Answered" asChild>
  <PermissionCard testid="permission" title="Calculator · calculate" {options} answer="Allow once" {onchoose} />
</Story>

<Story name="No options offered" asChild>
  <PermissionCard testid="permission" title="Bash" options={[]} {onchoose} />
</Story>

<!-- The options come from an agent: a repeated id shows once instead of crashing the list. -->
<Story
  name="Repeated option ids"
  asChild
  play={async ({ canvas, userEvent }) => {
    onchoose.mockClear()
    await expect(canvas.getAllByRole('button')).toHaveLength(2)
    await userEvent.click(canvas.getByTestId('permission-allow'))
    await expect(onchoose).toHaveBeenCalledWith('allow')
  }}
>
  <PermissionCard
    testid="permission"
    title="Bash"
    options={[
      { optionId: 'allow', name: 'Allow once', kind: 'allow_once' },
      { optionId: 'allow', name: 'Allow (again)', kind: 'allow_always' },
      { optionId: 'deny', name: 'Deny', kind: 'reject_once' }
    ]}
    {onchoose}
  />
</Story>
