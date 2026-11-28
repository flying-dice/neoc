<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import CopilotPage from './CopilotPage.svelte'
  import { Copilots } from '../lib/copilot.svelte'
  import { ADMIN, CODER, UNREACHABLE, conversation, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  const TALK = conversation('a', 'What is 12 times 7?', CODER, [
    ['user', 'What is 12 times 7?'],
    ['assistant', '12 times 7 is 84.']
  ])
  const withChats = (): FakeBackend => createFakeBackend({ chats: [TALK, conversation('b', 'Is the DNS up?', ADMIN, [['user', 'Is the DNS up?']])] })

  /** The Copilot app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Copilot',
    component: CopilotPage,
    tags: ['app'],
    args: { width: app.window.w, height: app.window.h, backend: () => createFakeBackend() },
    argTypes: { backend: { control: false } },
    render: template
  })

  type Canvas = { getByTestId(id: string): HTMLElement; queryByTestId(id: string): HTMLElement | null }
  type User = {
    click(element: Element): Promise<void>
    type(element: Element, text: string, options?: { initialSelectionStart: number; initialSelectionEnd: number }): Promise<void>
  }
  /** The kit's text field holds the real input in its shadow tree. */
  const field = (canvas: Canvas, id: string): HTMLInputElement | HTMLTextAreaElement => canvas.getByTestId(id).shadowRoot!.querySelector('input, textarea')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const said = (canvas: Canvas, text: string): Promise<void> => waitFor(() => expect(canvas.getByTestId('copilot-thread')).toHaveTextContent(text))

  async function say(canvas: Canvas, user: User, text: string): Promise<void> {
    // the testing library keeps its own idea of what the field holds, and misses the app emptying it after a send: type
    // over whatever it thinks is there
    await user.type(field(canvas, 'copilot-composer-input'), `${text}{Enter}`, { initialSelectionStart: 0, initialSelectionEnd: 1000 })
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const copilots = new Copilots(backend.rpc, backend.events)}
  <AppWindow title={app.title} width={args.width} height={args.height} minWidth={app.window.minW} minHeight={app.window.minH}>
    <CopilotPage {copilots} onlink={() => {}} />
  </AppWindow>
{/snippet}

<!-- The copilots, each with its new chat and settings, and the library under them. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await appears(canvas, 'copilot-new-chat-coder')
    await expect(canvas.getByTestId('copilot-new-chat-admin')).toBeVisible()
    await expect(canvas.getByTestId('copilot-open-skills')).toBeVisible()
    await expect(canvas.getByTestId('copilot-no-chat')).toBeInTheDocument()
  }}
/>

<!-- No copilot yet: what to do. -->
<Story
  name="Nothing yet"
  args={{ backend: () => createFakeBackend({ copilots: [], skills: [] }) }}
  play={async ({ canvas }) => {
    await appears(canvas, 'copilot-add-first')
    await expect(canvas.getByTestId('copilot-no-chat')).toHaveProperty('message', 'Add a copilot: a Dockerfile and the command of the agent it runs.')
  }}
/>

<!-- Docker cannot be reached: why, and the field that tries again or chooses another. -->
<Story
  name="Docker not reachable"
  args={{ backend: () => createFakeBackend({ engine: UNREACHABLE }) }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-down')
    await userEvent.type(field(canvas, 'copilot-host-input'), 'tcp://docker:2375')
    await userEvent.click(canvas.getByTestId('copilot-host-connect'))
    await appears(canvas, 'copilot-new-chat-coder')
  }}
/>

<!-- Chats from before are listed under their copilots. -->
<Story
  name="Chats from before"
  args={{ backend: withChats }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-chat-a')
    await expect(canvas.getByTestId('copilot-copilot-coder')).toContainElement(canvas.getByTestId('copilot-chat-a'))
    await expect(canvas.getByTestId('copilot-copilot-admin')).toContainElement(canvas.getByTestId('copilot-chat-b'))
    await userEvent.click(canvas.getByTestId('copilot-chat-a'))
    await said(canvas, '12 times 7 is 84.')
    await expect(canvas.getByTestId('copilot-chat-a')).toHaveAttribute('selected', '')
  }}
/>

<!-- A new chat, a message, the agent's answer: the first message makes the chat, and names it. -->
<Story
  name="A first message answered"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-new-chat-coder')
    await userEvent.click(canvas.getByTestId('copilot-new-chat-coder'))
    await appears(canvas, 'copilot-composer-input')
    await expect(canvas.getByTestId('copilot-new-chat-coder')).toHaveAttribute('selected', '')
    await say(canvas, userEvent, 'hello')
    await said(canvas, 'Hi there')
    await waitFor(() => expect(canvas.getByTestId('copilot-chat-chat-1')).toHaveProperty('heading', 'hello'))
  }}
/>

<!-- The agent asks before it goes on: the answer goes to it, and it goes on; the answer can be stopped. -->
<Story
  name="A permission answered"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-new-chat-coder')
    await userEvent.click(canvas.getByTestId('copilot-new-chat-coder'))
    await appears(canvas, 'copilot-composer-input')
    await say(canvas, userEvent, 'ask')
    await appears(canvas, 'copilot-question-2-option-0')
    await expect(canvas.getByTestId('copilot-composer-stop')).toBeVisible()
    await userEvent.click(canvas.getByTestId('copilot-question-2-option-0'))
    await said(canvas, 'Added.')
    await appears(canvas, 'copilot-composer-send')
  }}
/>

<!-- Deleting a chat asks first, and says its container goes with it. -->
<Story
  name="Deleting a chat"
  args={{ backend: withChats }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-chat-a')
    await userEvent.click(canvas.getByTestId('copilot-chat-a'))
    await appears(canvas, 'copilot-remove-chat')
    await userEvent.click(canvas.getByTestId('copilot-remove-chat'))
    await waitFor(() => expect(alertDialog(canvas.getByTestId('copilot-confirm'))).toHaveTextContent('its container and data are deleted'))
    await userEvent.click(alertButton(canvas.getByTestId('copilot-confirm'), 'action'))
    await waitFor(() => expect(canvas.queryByTestId('copilot-chat-a')).toBeNull())
  }}
/>

<!-- A copilot's settings: everything it is, and its image, which is built from the form. -->
<Story
  name="Building a copilot"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-edit-admin')
    await userEvent.click(canvas.getByTestId('copilot-edit-admin'))
    await appears(canvas, 'copilot-form')
    await expect(field(canvas, 'copilot-form-command')).toHaveValue('agent --acp')
    await expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Not built yet')
    await userEvent.click(canvas.getByTestId('copilot-form-build'))
    await waitFor(() => expect(canvas.getByTestId('copilot-build-status')).toHaveTextContent('Built: vos-copilot-admin:'))
    await expect(canvas.getByTestId('copilot-build-lines')).toHaveTextContent('Step 2/2 : RUN apt-get update')
  }}
/>

<!-- Removing a copilot asks first, and says what goes with it. -->
<Story
  name="Removing a copilot"
  tags={['play-only']}
  args={{ backend: withChats }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-edit-admin')
    await userEvent.click(canvas.getByTestId('copilot-edit-admin'))
    await appears(canvas, 'copilot-form-remove')
    await userEvent.click(canvas.getByTestId('copilot-form-remove'))
    await waitFor(() => expect(alertDialog(canvas.getByTestId('copilot-confirm'))).toHaveTextContent('its images are removed with it'))
    await userEvent.click(alertButton(canvas.getByTestId('copilot-confirm'), 'action'))
    await waitFor(() => expect(canvas.queryByTestId('copilot-copilot-admin')).toBeNull())
    await expect(canvas.queryByTestId('copilot-chat-b')).toBeNull()
  }}
/>

<!-- The skill library: a skill is added, and is a choice in the copilot form. -->
<Story
  name="Adding a skill"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'copilot-open-skills')
    await userEvent.click(canvas.getByTestId('copilot-open-skills'))
    await appears(canvas, 'copilot-add-skill')
    await userEvent.click(canvas.getByTestId('copilot-add-skill'))
    await appears(canvas, 'copilot-skill-form')
    await userEvent.type(field(canvas, 'copilot-skill-form-name'), 'deploy')
    await userEvent.type(field(canvas, 'copilot-skill-form-text'), '# Deploy')
    await userEvent.click(canvas.getByTestId('copilot-skill-form-save'))
    await appears(canvas, 'copilot-skill-skill-1')
    await userEvent.click(canvas.getByTestId('copilot-edit-coder'))
    await appears(canvas, 'copilot-form-skill-skill-1')
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('copilot-failure')).toHaveTextContent('The host is not answering'))
  }}
/>
