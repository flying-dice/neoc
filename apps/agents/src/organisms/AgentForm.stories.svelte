<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import AgentForm from './AgentForm.svelte'
  import type { AgentForm as Form } from '../lib/agents.svelte'
  import { SCRIPTED } from '../lib/fakeApi'

  interface Args {
    form: Form
    failure?: string | null
    savable: boolean
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()

  const EMPTY: Form = { agent: null, name: '', url: '', token: '', forget: false }
  const KNOWN: Form = { agent: { ...SCRIPTED, token: true }, name: SCRIPTED.name, url: SCRIPTED.url, token: '', forget: false }

  /** The form an agent is added or changed in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/AgentForm',
    component: AgentForm,
    tags: ['autodocs'],
    args: { form: EMPTY, savable: false, onsave, oncancel, onremove },
    beforeEach: () => {
      onsave.mockClear()
      oncancel.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:520px;color:var(--label);font:var(--type-body)">
    <AgentForm {...args} />
  </div>
{/snippet}

<!-- A new agent: nothing to add until it has a name and an endpoint. -->
<Story
  name="A new agent"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.queryByTestId('agents-form-remove')).toBeNull()
  }}
/>

<!-- An agent that is there: its token is not shown, only that it has one, with the ways to replace or remove it. -->
<Story
  name="An agent with a token"
  args={{ form: KNOWN, savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-form-token').shadowRoot!.querySelector('input')).toHaveValue('')
    await expect(canvas.getByTestId('agents-form-forget')).toBeVisible()
    await userEvent.click(canvas.getByTestId('agents-form-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('agents-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('agents-form-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<!-- The backend refused what was typed: its reason, under the fields. -->
<Story
  name="Refused"
  args={{ form: { ...EMPTY, name: 'Ada', url: 'ftp://example.com' }, savable: true, failure: 'The endpoint must be an http:// or https:// URL.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-agent')).toHaveProperty('footer', 'The endpoint must be an http:// or https:// URL.')
  }}
/>
