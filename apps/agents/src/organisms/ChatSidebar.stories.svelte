<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Agent = VosApp.Agents.Agent
  type Chat = VosApp.Agents.Chat
  import ChatSidebar from './ChatSidebar.svelte'
  import { CODER, ECHO, PICTURE, PICTURED, SCRIPTED } from '../lib/fakeApi'

  interface Args {
    agents: readonly Agent[]
    chats: readonly Chat[]
    selected?: string
    onstart: (agent: Agent) => void
    onedit: (agent: Agent) => void
    onadd: () => void
    onopen: (chat: Chat) => void
  }

  const onstart = fn<(agent: Agent) => void>()
  const onedit = fn<(agent: Agent) => void>()
  const onadd = fn<() => void>()
  const onopen = fn<(chat: Chat) => void>()

  const chat = (id: string, title: string, agent: Agent = SCRIPTED): Chat => ({ id, title, agent: agent.name, agent_id: agent.id, updated: '2026-06-09T08:41:00.000Z' })
  const CHATS = [chat('a', 'Plan the week'), chat('b', 'What is 12 times 7?'), chat('c', 'A title long enough that it has to be cut short in the list')]

  /** The agents and the chats. It keeps nothing: an agent tapped and a chat tapped go out. */
  const { Story } = defineMeta({
    title: 'Organisms/ChatSidebar',
    component: ChatSidebar,
    tags: ['autodocs'],
    args: { agents: [SCRIPTED], chats: [], onstart, onedit, onadd, onopen },
    beforeEach: () => {
      onstart.mockClear()
      onedit.mockClear()
      onadd.mockClear()
      onopen.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:300px;height:460px;color:var(--label);font:var(--type-body)">
    <ChatSidebar {...args} />
  </div>
{/snippet}

<!-- No chat yet: the agents, and what to do. -->
<Story
  name="No chats"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-agent-scripted')).toBeVisible()
    await expect(canvas.getByTestId('agents-no-chats')).toBeVisible()
  }}
/>

<Story
  name="With chats"
  args={{ chats: CHATS, selected: 'b' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-chat-b')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('agents-chat-a')).not.toHaveAttribute('selected')
  }}
/>

<!-- Nobody to chat with yet: the way to add an agent. -->
<Story
  name="No agents"
  args={{ agents: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-add')).toBeVisible()
  }}
/>

<!-- An agent with a picture shows it, in its row and in its chats'; one without shows its initials on a tint of its own. -->
<Story
  name="Several agents"
  args={{ agents: [SCRIPTED, PICTURED], chats: [chat('w', 'Write a poem', PICTURED), CHATS[0]!] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-avatar-agent-writer').querySelector('img')).toHaveAttribute('src', PICTURE)
    await expect(canvas.getByTestId('agents-avatar-chat-w').querySelector('img')).toHaveAttribute('src', PICTURE)
    await expect(canvas.getByTestId('agents-avatar-agent-scripted').querySelector('[role="img"]')).toHaveAttribute('data-initials', 'SA')
    await expect(canvas.getByTestId('agents-avatar-chat-a').querySelector('[role="img"]')).toHaveAttribute('data-initials', 'SA')
  }}
/>

<!-- Agents of the three types: under its name, a remote agent's address, Container, or a program's command. -->
<Story
  name="Agents of every type"
  args={{ agents: [CODER, ECHO, SCRIPTED] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-agent-scripted')).toHaveProperty('subheading', SCRIPTED.url)
    await expect(canvas.getByTestId('agents-agent-coder')).toHaveProperty('subheading', 'Container')
    await expect(canvas.getByTestId('agents-agent-echo')).toHaveProperty('subheading', 'python3 -u echo_agent.py')
  }}
/>

<!-- A chat with an agent that was removed: its initials, from the name it had. -->
<Story name="A removed agent's chat" args={{ chats: [chat('r', 'An old chat', { ...PICTURED, id: 'gone', name: 'Gone away' })] }} />

<Story
  name="Tapping"
  tags={['play-only']}
  args={{ chats: CHATS }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('agents-agent-scripted'))
    await expect(onstart).toHaveBeenCalledWith(SCRIPTED)
    await userEvent.click(canvas.getByTestId('agents-chat-a'))
    await expect(onopen).toHaveBeenCalledWith(CHATS[0])
    // the button on an agent's row changes the agent, and does not start a chat with it
    onstart.mockClear()
    await userEvent.click(canvas.getByTestId('agents-edit-scripted'))
    await expect(onedit).toHaveBeenCalledWith(SCRIPTED)
    await expect(onstart).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('agents-add'))
    await expect(onadd).toHaveBeenCalledTimes(1)
  }}
/>
