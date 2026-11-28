<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import AgentForm from './AgentForm.svelte'
  import type { AgentForm as Form, Choices } from '../lib/agents.svelte'
  import { CODER, ECHO, IMAGES, NO_SKILLS, PICTURE, PICTURED, SCRIPTED, SECRETS, SKILLS } from '../lib/fakeApi'
  import { agentDraftOf } from '../lib/form'
  type Secret = VosApp.Agents.Secret
  type SkillSummary = VosApp.Agents.SkillSummary

  interface Args {
    form: Form
    failure?: string | null
    savable: boolean
    secrets?: Choices<Secret>
    skills?: Choices<SkillSummary>
    images?: Choices<string>
    onsave: () => void
    oncancel: () => void
    onremove: () => void
  }

  const REFUSED = 'The address must be a ws:// or wss:// URL, like ws://example.com:5424/acp.'
  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()

  const EMPTY: Form = agentDraftOf(null, 'remote')
  const KNOWN: Form = agentDraftOf({ ...SCRIPTED, token: true })
  const formOf = (agent: typeof PICTURED): Form => agentDraftOf(agent)
  const CHOICES = {
    secrets: { items: SECRETS, problem: null },
    skills: { items: SKILLS, problem: null },
    images: { items: IMAGES, problem: null }
  }

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

<!-- A container agent: its image (typed, or one of Docker's), command and skills folder, its environment with a Keychain item, an MCP server, and the Skills app's skills it uses. -->
<Story
  name="A container agent"
  args={{ form: formOf(CODER), savable: true, ...CHOICES }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-form-kind')).toHaveProperty('value', 'Container')
    await expect(canvas.queryByTestId('agents-form-url')).toBeNull()
    await expect(canvas.queryByTestId('agents-form-token')).toBeNull()
    await expect(canvas.getByTestId('agents-form-image').shadowRoot!.querySelector('input')).toHaveValue(CODER.image)
    await expect(canvas.getByTestId('agents-form-image-choice')).toBeVisible()
    await expect(canvas.getByTestId('agents-form-skill-review')).toHaveProperty('checked', true)
    await expect(canvas.getByTestId('agents-form-skill-triage')).toHaveProperty('checked', false)
    await expect(canvas.getByTestId('agents-form-server-0')).toBeVisible()
    await userEvent.click(canvas.getByTestId('agents-form-add-env'))
    await expect(canvas.getByTestId('agents-form-env-1')).toBeVisible()
  }}
/>

<!-- A container agent while the Skills app is not there: why there are none to choose, and the skill it uses stays. -->
<Story
  name="A container agent without the Skills app"
  args={{ form: formOf(CODER), savable: true, ...CHOICES, skills: { items: [], problem: NO_SKILLS } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-skills')).toHaveProperty('footer', NO_SKILLS)
    await expect(canvas.getByTestId('agents-form-skill-review')).toHaveProperty('checked', true)
  }}
/>

<!-- A program agent: its command and the folder of this machine it works in, its environment and MCP servers. -->
<Story
  name="A program agent"
  args={{ form: formOf(ECHO), savable: true, ...CHOICES }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-form-kind')).toHaveProperty('value', 'Program')
    await expect(canvas.getByTestId('agents-form-command').shadowRoot!.querySelector('input')).toHaveValue('python3 -u echo_agent.py')
    await expect(canvas.getByTestId('agents-form-folder').shadowRoot!.querySelector('input')).toHaveValue(ECHO.folder)
    await expect(canvas.queryByTestId('agents-form-image')).toBeNull()
    await expect(canvas.queryByTestId('agents-form-skills')).toBeNull()
    await expect(canvas.getByTestId('agents-form-env-0')).toBeVisible()
  }}
/>

<!-- A new program agent: nothing to add until it has a name, a command and a folder. -->
<Story name="A new program agent" args={{ form: agentDraftOf(null, 'program'), savable: false, ...CHOICES }} />
