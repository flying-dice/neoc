<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import CopilotSidebar from './CopilotSidebar.svelte'
  import { ADMIN, CODER, conversation } from '../lib/fakeApi'

  type Chat = VosApp.Copilot.Chat
  type Copilot = VosApp.Copilot.Copilot

  interface Args {
    copilots: readonly Copilot[]
    chats: readonly Chat[]
    selected?: string
    onstart: (copilot: Copilot) => void
    onedit: (copilot: Copilot | null) => void
    onopen: (chat: Chat) => void
    onlibrary: () => void
  }

  const onstart = fn<(copilot: Copilot) => void>()
  const onedit = fn<(copilot: Copilot | null) => void>()
  const onopen = fn<(chat: Chat) => void>()
  const onlibrary = fn<() => void>()

  const CHATS = [conversation('a', 'What is 12 times 7?', CODER, []).chat, conversation('b', 'Is the DNS up?', ADMIN, []).chat, conversation('c', 'Fix the tests', CODER, []).chat]

  /** The copilots and their chats. */
  const { Story } = defineMeta({
    title: 'Organisms/CopilotSidebar',
    component: CopilotSidebar,
    tags: ['autodocs'],
    args: { copilots: [CODER, ADMIN], chats: CHATS, onstart, onedit, onopen, onlibrary },
    beforeEach: () => {
      onstart.mockClear()
      onedit.mockClear()
      onopen.mockClear()
      onlibrary.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:280px;height:560px;color:var(--label);font:var(--type-body)">
    <CopilotSidebar {...args} />
  </div>
{/snippet}

<!-- Each copilot with its new chat, its settings and its chats; then adding one and the library. -->
<Story
  name="Copilots and chats"
  args={{ selected: 'chat:a' }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('copilot-copilot-coder')).toContainElement(canvas.getByTestId('copilot-chat-c'))
    await expect(canvas.getByTestId('copilot-chat-a')).toHaveAttribute('selected', '')
    await userEvent.click(canvas.getByTestId('copilot-new-chat-admin'))
    await expect(onstart).toHaveBeenCalledWith(ADMIN)
    await userEvent.click(canvas.getByTestId('copilot-edit-coder'))
    await expect(onedit).toHaveBeenCalledWith(CODER)
    await userEvent.click(canvas.getByTestId('copilot-chat-b'))
    await expect(onopen).toHaveBeenCalledWith(CHATS[1])
    await userEvent.click(canvas.getByTestId('copilot-add'))
    await expect(onedit).toHaveBeenLastCalledWith(null)
    await userEvent.click(canvas.getByTestId('copilot-open-skills'))
    await expect(onlibrary).toHaveBeenCalledTimes(1)
  }}
/>

<!-- No copilot yet: the way to add one, and the library. -->
<Story name="Nothing yet" args={{ copilots: [], chats: [] }} />
