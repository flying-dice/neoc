<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Conversation = VosApp.Agents.Conversation
  type Item = VosApp.Agents.Item
  import type { PermissionOption } from '../lib/items'
  import ChatThread from './ChatThread.svelte'
  import { ASKING, OPTIONS, PLAN, TOOL, conversation } from '../lib/fakeApi'

  interface Args {
    conversation: Conversation
    draft: string
    failure?: string | null
    back?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    onanswer: (item: Item, option: PermissionOption) => void
    onremove: () => void
    onback?: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onsend = fn<() => void>()
  const onstop = fn<() => void>()
  const onanswer = fn<(item: Item, option: PermissionOption) => void>()
  const onremove = fn<() => void>()
  const onback = fn<() => void>()

  const TALK = conversation('a', 'What is 12 times 7?', [
    ['user', 'What is 12 times 7?'],
    ['assistant', '12 times 7 is 84.'],
    ['user', 'And divided by 4?']
  ])
  const answering = (text: string): Conversation => ({
    ...TALK,
    items: [...TALK.items, { id: 9, chat: 'a', kind: 'assistant', text, name: '', data: '', done: false }],
    busy: true
  })
  /** Everything an agent can put in a chat besides its words. */
  const WORK = conversation('w', 'Add milk to my list', [
    { kind: 'note', text: 'The agent cannot load an earlier session, so it started a new one: it does not remember the earlier messages.' },
    ['user', 'Add milk to my list'],
    { kind: 'reasoning', text: 'The person keeps a shopping list in list.md. I should add to it.' },
    PLAN,
    TOOL,
    { kind: 'tool', name: 'Search for prices', data: JSON.stringify({ kind: 'search', status: 'failed' }) },
    ['assistant', 'Milk is on your list.'],
    { kind: 'assistant', name: 'link', text: 'list.md', data: JSON.stringify({ type: 'link', uri: 'file:///home/ada/project/list.md', name: 'list.md' }) },
    { kind: 'note', text: 'The agent stopped: it reached the most it may write in one answer.' },
    { kind: 'error', text: 'The connection to the agent closed before it finished answering.' }
  ])
  /** The agent asks before it goes on: its turn is running, and waits for the answer. */
  const ASKED = { ...conversation('q', 'Add milk to my list', [['user', 'Add milk to my list'], ASKING]), busy: true }

  const long = conversation(
    'l',
    'A long chat',
    Array.from({ length: 24 }, (_, i): ['user' | 'assistant', string] => [i % 2 === 0 ? 'user' : 'assistant', `Line ${i + 1}`])
  )

  /** One chat: its title, what was said, the composer. It keeps nothing but where the log is scrolled to. */
  const { Story } = defineMeta({
    title: 'Organisms/ChatThread',
    component: ChatThread,
    tags: ['autodocs'],
    args: { conversation: TALK, draft: '', ondraft, onsend, onstop, onanswer, onremove, onback },
    beforeEach: () => {
      onback.mockClear()
      onanswer.mockClear()
      onremove.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:460px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ChatThread {...args} />
  </div>
{/snippet}

<Story
  name="A chat"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-title')).toHaveTextContent('What is 12 times 7?')
    await expect(canvas.getByTestId('agents-thread')).toHaveTextContent('12 times 7 is 84.')
    await expect(canvas.getByTestId('agents-composer-send')).toBeVisible()
  }}
/>

<!-- Just started: nothing said yet. -->
<Story name="Nothing said yet" args={{ conversation: conversation('n', 'New chat', []) }} />

<!-- The agent is answering: its message so far with a spinner, and the button stops it. -->
<Story
  name="An answer on its way"
  args={{ conversation: answering('84 divided by') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-composer-stop')).toBeVisible()
    await expect(canvas.getByTestId('agents-message-assistant-9')).toHaveTextContent('84 divided by')
  }}
/>

<!-- What an agent did: its reasoning and tools closed until asked for, its plan open, notes on the chat, and a turn that failed. -->
<Story
  name="What the agent did"
  args={{ conversation: WORK }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-note-1')).toHaveTextContent('started a new one')
    await expect(canvas.getByTestId('agents-item-plan-4')).toHaveTextContent('Add milk')
    await expect(canvas.getByTestId('agents-item-tool-5')).toHaveTextContent('Add milk to list.md')
    await userEvent.click(canvas.getByTestId('agents-item-tool-5-summary'))
    await expect(canvas.getByTestId('agents-item-5-diff-0')).toHaveTextContent('+Milk')
    await expect(canvas.getByTestId('agents-item-tool-6')).toHaveTextContent('failed')
    await expect(canvas.getByTestId('agents-message-assistant-8')).toHaveTextContent('list.md')
    await expect(canvas.getByTestId('agents-item-error-10')).toHaveTextContent('closed before it finished')
  }}
/>

<!-- The agent asks before it goes on: it is answered in the chat with one of its options, and its turn can be stopped. -->
<Story
  name="A permission the agent asks for"
  args={{ conversation: ASKED }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-question-2')).toHaveTextContent('Add milk to list.md')
    await expect(canvas.getByTestId('agents-composer-stop')).toBeVisible()
    await userEvent.click(canvas.getByTestId('agents-question-2-option-1'))
    await expect(onanswer).toHaveBeenCalledWith(ASKED.items[1], OPTIONS[1])
  }}
/>

<!-- The last call failed: the reason sits over the composer, and what was typed is still there. -->
<Story
  name="A message that was not sent"
  args={{ failure: 'The host is not answering', draft: 'And divided by 4?' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-failure')).toHaveTextContent('The host is not answering')
  }}
/>

<!-- More than fits: the log scrolls, and opens on the newest line. -->
<Story
  name="A long chat"
  args={{ conversation: long }}
  play={async ({ canvas }) => {
    const thread = canvas.getByTestId('agents-thread')
    await expect(thread.scrollHeight).toBeGreaterThan(thread.clientHeight)
    await expect(Math.ceil(thread.scrollTop + thread.clientHeight)).toBeGreaterThanOrEqual(thread.scrollHeight - 1)
  }}
/>

<Story
  name="Deleting the chat"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('agents-remove-chat'))
    await expect(onremove).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A window too narrow for the list as well: the way back to it. -->
<Story
  name="With the way back"
  args={{ back: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('agents-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
