<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Conversation = VosApp.Agents.Conversation
  type Item = VosApp.Agents.Item
  import type { UserAction } from '../lib/a2ui'
  import ChatThread from './ChatThread.svelte'
  import { SURFACE, conversation } from '../lib/fakeApi'

  interface Args {
    conversation: Conversation
    draft: string
    failure?: string | null
    back?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    onanswer: (item: Item, answer: string | null) => void
    onaction: (item: Item, action: UserAction) => void
    onremove: () => void
    onback?: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onsend = fn<() => void>()
  const onstop = fn<() => void>()
  const onanswer = fn<(item: Item, answer: string | null) => void>()
  const onaction = fn<(item: Item, action: UserAction) => void>()
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
    ['user', 'Add milk to my list'],
    { kind: 'reasoning', text: 'The person keeps a shopping list in Notes. I should add to it.' },
    { kind: 'step', name: 'plan' },
    { kind: 'tool', name: 'notes__add', text: '{"title":"Milk"}', data: JSON.stringify({ ours: true, result: '{"id":12}' }) },
    { kind: 'tool', name: 'web_search', text: '{"q":"milk"}', data: JSON.stringify({ ours: false }), done: false },
    { kind: 'subagent', name: 'researcher', text: 'Looks for prices', data: JSON.stringify({ error: 'It ran out of time.' }) },
    { kind: 'activity', name: 'PLAN', data: JSON.stringify({ steps: ['find the list', 'add milk'] }) },
    { kind: 'custom', name: 'usage', data: JSON.stringify({ tokens: 412 }) },
    ['assistant', 'Milk is on your list.'],
    { kind: 'error', text: "The agent's answer ended before it was finished." }
  ])
  const ASKED = conversation('q', 'Send the report', [
    ['user', 'Send the report'],
    { kind: 'interrupt', text: 'Send the report to everyone?', name: 'approval', data: JSON.stringify({ status: 'open' }), done: false }
  ])
  const DRAWN = conversation('s', 'Book a table', [
    ['user', 'Book a table'],
    { kind: 'activity', name: 'a2ui-surface', data: JSON.stringify(SURFACE) }
  ])
  const field = (canvas: { getByTestId(id: string): HTMLElement }, id: string): HTMLInputElement =>
    canvas.getByTestId(id).shadowRoot!.querySelector('input')!

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
    args: { conversation: TALK, draft: '', ondraft, onsend, onstop, onanswer, onaction, onremove, onback },
    beforeEach: () => {
      onback.mockClear()
      onanswer.mockClear()
      onaction.mockClear()
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

<!-- What an agent did: its reasoning and tools closed until asked for, an activity it reported, and a run that failed. -->
<Story
  name="What the agent did"
  args={{ conversation: { ...WORK, state: '{"list":["Milk"]}' } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-tool-4')).toHaveTextContent('notes__add')
    await expect(canvas.getByTestId('agents-item-tool-4')).toHaveTextContent('an app on this machine')
    await userEvent.click(canvas.getByTestId('agents-item-tool-4-summary'))
    await expect(canvas.getByTestId('agents-item-tool-4')).toHaveTextContent('"title": "Milk"')
    await expect(canvas.getByTestId('agents-item-subagent-6')).toHaveTextContent('It ran out of time.')
    await expect(canvas.getByTestId('agents-item-activity-7')).toHaveTextContent('find the list')
    await expect(canvas.getByTestId('agents-item-error-10')).toHaveTextContent('ended before it was finished')
    await expect(canvas.getByTestId('agents-state')).toHaveTextContent('State the agent shares')
  }}
/>

<!-- The agent stopped at a question: it is answered or declined in the chat, and nothing else is sent until then. -->
<Story
  name="A question from the agent"
  args={{ conversation: ASKED }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-question-2')).toHaveTextContent('Send the report to everyone?')
    await expect(canvas.getByTestId('agents-composer-send')).toHaveProperty('disabled', true)
    await userEvent.type(field(canvas, 'agents-question-2-input'), 'Only to Ada')
    await userEvent.click(canvas.getByTestId('agents-question-2-answer'))
    await expect(onanswer).toHaveBeenCalledWith(ASKED.items[1], 'Only to Ada')
    await userEvent.click(canvas.getByTestId('agents-question-2-decline'))
    await expect(onanswer).toHaveBeenCalledWith(ASKED.items[1], null)
  }}
/>

<!-- A surface the agent drew (A2UI): what is typed in it changes its text at once, and its button tells the agent. -->
<Story
  name="A surface the agent drew"
  args={{ conversation: DRAWN }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-surface-booking')).toHaveTextContent('A table for 2, for Ada.')
    await userEvent.type(field(canvas, 'agents-a2ui-booking-who'), ' L')
    await expect(canvas.getByTestId('agents-surface-booking')).toHaveTextContent('A table for 2, for Ada L.')
    await userEvent.click(canvas.getByTestId('agents-a2ui-booking-confirm'))
    await expect(onaction).toHaveBeenCalledWith(
      DRAWN.items[1],
      expect.objectContaining({ name: 'confirm', surfaceId: 'booking', sourceComponentId: 'confirm', context: { name: 'Ada L' } })
    )
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
