<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import CopilotForm from './CopilotForm.svelte'
  import type { Build } from '../lib/copilot.svelte'
  import { copilotDraftOf, type CopilotDraft } from '../lib/form'
  import { ADMIN, CODER, NO_KEYCHAIN, REVIEW, SECRETS } from '../lib/fakeApi'

  type Copilot = VosApp.Copilot.Copilot
  type Secret = VosApp.Copilot.Secret
  type Skill = VosApp.Copilot.Skill

  interface Args {
    form: CopilotDraft
    copilot: Copilot | null
    skills: readonly Skill[]
    secrets: readonly Secret[]
    secretsProblem?: string | null
    build: Build | null
    failure?: string | null
    savable: boolean
    onsave: () => void
    onbuild: () => void
    oncancel: () => void
    onremove: () => void
  }

  const onsave = fn<() => void>()
  const onbuild = fn<() => void>()
  const oncancel = fn<() => void>()
  const onremove = fn<() => void>()

  const BUILDING: Build = { lines: ['Step 1/2 : FROM debian:stable', 'Step 2/2 : RUN apt-get update && apt-get install -y curl dnsutils'], status: 'running' }
  const FAILED: Build = { lines: ['Step 1/2 : FROM debian:stable', "The command '/bin/sh -c false' returned a non-zero code: 1"], status: 'failed' }

  /** The form a copilot is added or changed in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/CopilotForm',
    component: CopilotForm,
    tags: ['autodocs'],
    args: { form: copilotDraftOf(null), copilot: null, skills: [REVIEW], secrets: SECRETS, build: null, savable: false, onsave, onbuild, oncancel, onremove },
    beforeEach: () => {
      onsave.mockClear()
      onbuild.mockClear()
      oncancel.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:640px;color:var(--label);font:var(--type-body)">
    <CopilotForm {...args} />
  </div>
{/snippet}

<!-- A new copilot: the usual folders, nothing to add until it has a name, a Dockerfile and a command, and nothing to remove. -->
<Story
  name="A new copilot"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('copilot-form-workdir')).toHaveProperty('value', '/workspace')
    await expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Not built yet')
    await expect(canvas.queryByTestId('copilot-form-remove')).toBeNull()
  }}
/>

<!-- A copilot as it is: its command as words, a variable from the Keychain, a server, its skill, and its image built. -->
<Story
  name="A copilot"
  args={{ form: copilotDraftOf(CODER), copilot: CODER, savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('copilot-form-command')).toHaveProperty('value', 'claude-code-acp')
    await expect(canvas.getByTestId('copilot-form-env-0-source')).toHaveProperty('value', JSON.stringify(['anthropic', 'ada']))
    await expect(canvas.queryByTestId('copilot-form-env-0-value')).toBeNull()
    await expect(canvas.getByTestId('copilot-form-server-0-args')).toHaveProperty('value', '--root /workspace')
    await expect(canvas.getByTestId('copilot-form-skill-review')).toHaveProperty('checked', true)
    await expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Built: vos-copilot-coder:1a2b3c')
    await userEvent.click(canvas.getByTestId('copilot-form-build'))
    await expect(onbuild).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('copilot-form-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Its image is being built: the lines so far, and no second build. -->
<Story
  name="Building"
  args={{ form: copilotDraftOf(ADMIN), copilot: ADMIN, savable: true, build: BUILDING }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Building…')
    await expect(canvas.getByTestId('copilot-build-lines')).toHaveTextContent('Step 2/2')
    await expect(canvas.getByTestId('copilot-form-build')).toHaveProperty('disabled', true)
  }}
/>

<!-- The build failed: why, at the end of its lines. -->
<Story
  name="A build that failed"
  args={{ form: copilotDraftOf(ADMIN), copilot: { ...ADMIN, problem: FAILED.lines[1]! }, savable: true, build: FAILED }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Failed: The command')
  }}
/>

<!-- The Keychain app is not there: why, and values are written out. -->
<Story
  name="Without the Keychain"
  args={{ form: copilotDraftOf(ADMIN), copilot: ADMIN, savable: true, secrets: [], secretsProblem: NO_KEYCHAIN }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-form-env')).toHaveProperty('footer', NO_KEYCHAIN)
    await expect(canvas.queryByTestId('copilot-form-env-0-source')).toBeNull()
    await expect(canvas.getByTestId('copilot-form-env-0-value')).toHaveProperty('value', 'en_GB.UTF-8')
  }}
/>

<!-- The backend refused the form: why, over the fields. -->
<Story
  name="Refused"
  args={{ form: { ...copilotDraftOf(ADMIN), workdir: 'work' }, copilot: ADMIN, savable: true, failure: 'work is not an absolute path.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('copilot-form-failure')).toHaveTextContent('work is not an absolute path.')
  }}
/>
