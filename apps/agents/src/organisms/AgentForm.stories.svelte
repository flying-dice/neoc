<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import AgentForm from './AgentForm.svelte'
  import type { AgentForm as Form } from '../lib/agents.svelte'
  import { PICTURE, PICTURED, SCRIPTED } from '../lib/fakeApi'

  interface Args {
    form: Form
    failure?: string | null
    savable: boolean
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  const REFUSED = 'The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp.'
  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()

  const EMPTY: Form = { agent: null, name: '', url: '', folder: '/', token: '', forget: false, avatar: '' }
  const KNOWN: Form = { agent: { ...SCRIPTED, token: true }, name: SCRIPTED.name, url: SCRIPTED.url, folder: SCRIPTED.folder, token: '', forget: false, avatar: '' }
  const formOf = (agent: typeof PICTURED): Form => ({ agent, name: agent.name, url: agent.url, folder: agent.folder, token: '', forget: false, avatar: agent.avatar })

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

<!-- A new agent: nothing to add until it has a name and an address. -->
<Story
  name="A new agent"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.queryByTestId('agents-form-remove')).toBeNull()
    // no picture yet: the way to choose one, and nothing to remove
    await expect(canvas.getByTestId('agents-form-choose-picture')).toHaveTextContent('Choose…')
    await expect(canvas.queryByTestId('agents-form-remove-picture')).toBeNull()
  }}
/>

<!-- An agent with a picture: it is shown, with the ways to change it and to remove it; removed, the initials are. -->
<Story
  name="An agent with a picture"
  args={{ form: formOf(PICTURED), savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByRole('img', { name: PICTURED.name })).toHaveAttribute('src', PICTURE)
    await expect(canvas.getByTestId('agents-form-choose-picture')).toHaveTextContent('Change…')
    await userEvent.click(canvas.getByTestId('agents-form-remove-picture'))
    await expect(canvas.getByRole('img', { name: PICTURED.name })).toHaveAttribute('data-initials', 'W')
    await expect(canvas.queryByTestId('agents-form-remove-picture')).toBeNull()
  }}
/>

<!-- An agent that is there: its token is not shown, only that it has one, with the ways to replace or remove it. -->
<Story
  name="An agent with a token"
  args={{ form: KNOWN, savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-form-token').shadowRoot!.querySelector('input')).toHaveValue('')
    await expect(canvas.getByTestId('agents-form-folder').shadowRoot!.querySelector('input')).toHaveValue(SCRIPTED.folder)
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
  args={{ form: { ...EMPTY, name: 'Ada', url: 'https://example.com/agent' }, savable: true, failure: REFUSED }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-agent')).toHaveProperty('footer', REFUSED)
  }}
/>
