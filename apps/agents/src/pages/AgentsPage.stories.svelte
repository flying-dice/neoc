<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, waitFor } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  import AppWindow from '@vos/storybook/AppWindow.svelte'
  import pkg from '../../package.json'
  import AgentsPage from './AgentsPage.svelte'
  import { Agents } from '../lib/agents.svelte'
  import { conversation, createFailingBackend, createFakeBackend, type FakeBackend } from '../lib/fakeApi'

  // the window the app asks for (package.json), and the smallest it allows
  const app = pkg.vos

  interface Args {
    width: number
    height: number
    /** The backend the story's window talks to: a fake in the page, never a host. A function, so each story has its own. */
    backend: () => FakeBackend
  }

  const TALK = conversation('a', 'What is 12 times 7?', [
    ['user', 'What is 12 times 7?'],
    ['assistant', '12 times 7 is 84.']
  ])
  const withChats = (): FakeBackend => createFakeBackend({ chats: [TALK, conversation('b', 'Plan the week', [['user', 'Plan the week']])] })

  /** The Agents app in its window, on a fake backend: every state a person can bring it to. */
  const { Story } = defineMeta({
    title: 'Pages/Agents',
    component: AgentsPage,
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
  const input = (canvas: Canvas): HTMLInputElement =>
    canvas.getByTestId('agents-composer-input').shadowRoot!.querySelector('input')!
  const field = (canvas: Canvas, id: string): HTMLInputElement => canvas.getByTestId(id).shadowRoot!.querySelector('input')!
  const appears = (canvas: Canvas, id: string): Promise<void> => waitFor(() => expect(canvas.getByTestId(id)).toBeVisible())
  const said = (canvas: Canvas, text: string): Promise<void> =>
    waitFor(() => expect(canvas.getByTestId('agents-thread')).toHaveTextContent(text))

  async function startChat(canvas: Canvas, user: User): Promise<void> {
    await appears(canvas, 'agents-agent-scripted')
    await user.click(canvas.getByTestId('agents-agent-scripted'))
    await appears(canvas, 'agents-composer-input')
  }
  async function say(canvas: Canvas, user: User, text: string): Promise<void> {
    // the testing library keeps its own idea of what the field holds, and misses the app emptying it after a send: type
    // over whatever it thinks is there
    await user.type(input(canvas), `${text}{Enter}`, { initialSelectionStart: 0, initialSelectionEnd: 1000 })
  }
</script>

{#snippet template(args: Args)}
  <!-- one state per story, as each window has one -->
  {@const backend = args.backend()}
  {@const agents = new Agents(backend.rpc, backend.events)}
  <AppWindow
    title={app.title}
    width={args.width}
    height={args.height}
    minWidth={app.window.minW}
    minHeight={app.window.minH}
  >
    <AgentsPage {agents} />
  </AppWindow>
{/snippet}

<!-- Nothing yet: who can be chatted with, and what to do. -->
<Story
  name="Just opened"
  play={async ({ canvas }) => {
    await appears(canvas, 'agents-agent-scripted')
    await expect(canvas.getByTestId('agents-no-chats')).toBeInTheDocument()
    await expect(canvas.getByTestId('agents-no-chat')).toBeInTheDocument()
  }}
/>

<!-- Nobody to chat with: an agent is added by its name and its AG-UI endpoint, and can be chatted with at once. -->
<Story
  name="Adding the first agent"
  args={{ backend: () => createFakeBackend({ agents: [] }) }}
  play={async ({ canvas, userEvent }) => {
    // the sidebar's own way to add one is there in a window of any width
    await appears(canvas, 'agents-add')
    await userEvent.click(canvas.getByTestId('agents-add'))
    await appears(canvas, 'agents-form')
    await expect(canvas.getByTestId('agents-form-save')).toHaveProperty('disabled', true)
    await userEvent.type(field(canvas, 'agents-form-name'), 'Ada')
    await userEvent.type(field(canvas, 'agents-form-url'), 'ftp://example.com')
    await userEvent.click(canvas.getByTestId('agents-form-save'))
    await waitFor(() => expect(canvas.getByTestId('agents-form-agent')).toHaveProperty('footer', 'The endpoint must be an http:// or https:// URL.'))
    await userEvent.type(field(canvas, 'agents-form-url'), 'https://example.com/agent', { initialSelectionStart: 0, initialSelectionEnd: 1000 })
    await userEvent.click(canvas.getByTestId('agents-form-save'))
    await appears(canvas, 'agents-agent-agent-1')
    await expect(canvas.queryByTestId('agents-form')).toBeNull()
  }}
/>

<!-- An agent's row has the way to change it: its name, endpoint and token, or to remove it. -->
<Story
  name="Changing an agent"
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'agents-edit-scripted')
    await userEvent.click(canvas.getByTestId('agents-edit-scripted'))
    await appears(canvas, 'agents-form-remove')
    await expect(field(canvas, 'agents-form-name')).toHaveValue('Scripted agent')
  }}
/>

<!-- Chats from before are listed; none is open until one is picked. -->
<Story
  name="Chats from before"
  args={{ backend: withChats }}
  play={async ({ canvas }) => {
    await appears(canvas, 'agents-chat-a')
    await expect(canvas.getByTestId('agents-chat-b')).toHaveProperty('heading', 'Plan the week')
  }}
/>

<Story
  name="A chat open"
  args={{ backend: withChats }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'agents-chat-a')
    await userEvent.click(canvas.getByTestId('agents-chat-a'))
    await said(canvas, '12 times 7 is 84.')
    await expect(canvas.getByTestId('agents-chat-a')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('agents-title')).toHaveTextContent('What is 12 times 7?')
  }}
/>

<!-- Deleting a chat asks first: Cancel keeps it, Delete takes it off the list. -->
<Story
  name="Deleting a chat"
  args={{ backend: withChats }}
  play={async ({ canvas, userEvent }) => {
    await appears(canvas, 'agents-chat-a')
    await userEvent.click(canvas.getByTestId('agents-chat-a'))
    await appears(canvas, 'agents-remove-chat')
    await userEvent.click(canvas.getByTestId('agents-remove-chat'))
    await waitFor(() => expect(alertDialog(canvas.getByTestId('agents-confirm'))).toHaveTextContent('Delete this chat?'))
    await userEvent.click(alertButton(canvas.getByTestId('agents-confirm'), 'cancel'))
    await waitFor(() => expect(canvas.queryByTestId('agents-confirm')).toBeNull())
    await userEvent.click(canvas.getByTestId('agents-remove-chat'))
    await appears(canvas, 'agents-confirm')
    await userEvent.click(alertButton(canvas.getByTestId('agents-confirm'), 'action'))
    await waitFor(() => expect(canvas.queryByTestId('agents-chat-a')).toBeNull())
    await expect(canvas.getByTestId('agents-chat-b')).toBeInTheDocument()
  }}
/>

<!-- A new chat, a message, the agent's answer: the first message names the chat. -->
<Story
  name="A first message answered"
  play={async ({ canvas, userEvent }) => {
    await startChat(canvas, userEvent)
    await say(canvas, userEvent, 'hello')
    await said(canvas, 'Hi there')
    await appears(canvas, 'agents-composer-send')
    await expect(canvas.getByTestId('agents-title')).toHaveTextContent('hello')
    await waitFor(() => expect(canvas.getByTestId('agents-chat-chat-1')).toHaveProperty('heading', 'hello'))
    await expect(input(canvas)).toHaveValue('')
  }}
/>

<!-- The agent is answering: what it has said so far, and the button stops it. -->
<Story
  name="An answer on its way"
  play={async ({ canvas, userEvent }) => {
    await startChat(canvas, userEvent)
    await say(canvas, userEvent, 'slow')
    await said(canvas, 'Thinking')
    await expect(canvas.getByTestId('agents-composer-stop')).toBeVisible()
  }}
/>

<!-- Stopped: what the agent said stays, and the chat takes the next message. -->
<Story
  name="Stopping an answer"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await startChat(canvas, userEvent)
    await say(canvas, userEvent, 'slow')
    await said(canvas, 'Thinking')
    await userEvent.click(canvas.getByTestId('agents-composer-stop'))
    await appears(canvas, 'agents-composer-send')
    await said(canvas, 'Thinking')
    await say(canvas, userEvent, 'hello')
    await said(canvas, 'Hi there')
  }}
/>

<!-- The backend did not answer (the host, the connection): the reason, and no agents to pick. -->
<Story
  name="The backend cannot be reached"
  args={{ backend: () => createFailingBackend('The host is not answering') }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(canvas.getByTestId('agents-failure')).toHaveTextContent('The host is not answering'))
    await expect(canvas.queryByTestId('agents-agent-scripted')).toBeNull()
  }}
/>
