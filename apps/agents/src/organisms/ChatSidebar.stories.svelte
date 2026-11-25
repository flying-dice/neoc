<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Agent, Chat } from '@agents/rpc'
  import ChatSidebar from './ChatSidebar.svelte'
  import { SCRIPTED } from '../lib/fakeApi'

  interface Args {
    agents: readonly Agent[]
    chats: readonly Chat[]
    selected?: string
    onstart: (agent: Agent) => void
    onedit: (agent: Agent | null) => void
    onopen: (chat: Chat) => void
  }

  const onstart = fn<(agent: Agent) => void>()
  const onedit = fn<(agent: Agent | null) => void>()
  const onopen = fn<(chat: Chat) => void>()

  const chat = (id: string, title: string): Chat => ({ id, title, agent: SCRIPTED.name, updated: '2026-06-09T08:41:00.000Z' })
  const CHATS = [chat('a', 'Plan the week'), chat('b', 'What is 12 times 7?'), chat('c', 'A title long enough that it has to be cut short in the list')]

  /** The agents and the chats. It keeps nothing: an agent tapped and a chat tapped go out. */
  const { Story } = defineMeta({
    title: 'Organisms/ChatSidebar',
    component: ChatSidebar,
    tags: ['autodocs'],
    args: { agents: [SCRIPTED], chats: [], onstart, onedit, onopen },
    beforeEach: () => {
      onstart.mockClear()
      onedit.mockClear()
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

<Story
  name="Several agents"
  args={{
    agents: [SCRIPTED, { id: 'writer', name: 'Writer', url: 'https://writer.example.com/agui', token: true }],
    chats: CHATS.slice(0, 1)
  }}
/>

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
    await expect(onedit).toHaveBeenCalledWith(null)
  }}
/>
