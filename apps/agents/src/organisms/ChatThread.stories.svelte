<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Conversation = VosApp.Agents.Conversation
  type Item = VosApp.Agents.Item
  import type { PermissionOption } from '../lib/items'
  import ChatThread from './ChatThread.svelte'
  import { ASKING, OPTIONS, PICTURE, PICTURED, PLAN, TOOL, conversation } from '../lib/fakeApi'

  interface Args {
    conversation: Conversation
    avatar?: string
    draft: string
    failure?: string | null
    back?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    onanswer: (item: Item, option: PermissionOption) => void
    onremove: () => void
    onback?: () => void
    onlink: (url: string) => void
  }

  const ondraft = fn<(text: string) => void>()
  const onsend = fn<() => void>()
  const onstop = fn<() => void>()
  const onanswer = fn<(item: Item, option: PermissionOption) => void>()
  const onremove = fn<() => void>()
  const onback = fn<() => void>()
  const onlink = fn<(url: string) => void>()

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

  /** The agent at work, as a phone shows it: a reply in Markdown, two tools (one opened), and another still running. */
  const BUSY: Conversation = {
    ...conversation('p', 'Vos projects', [
      ['user', 'Vos projects'],
      { kind: 'tool', name: 'ToolSearch', data: JSON.stringify({ kind: 'search', status: 'completed', input: '{"query":"vos projects"}', output: '{"tools":["vos:projects__list_issues"]}' }) },
      {
        kind: 'tool',
        name: 'vos:projects__list_issues',
        data: JSON.stringify({ kind: 'other', status: 'completed', input: '{"project":"NEOC"}', content: [{ type: 'text', text: 'NEOC-1  Make a page  Task  medium  done' }] })
      },
      ['assistant', "Yes. There's one VOS project.\n\n- **NEOC** (key `NEOC`)\n  - `NEOC-1`: \"Make a page\". Task, medium priority, done.\n\nI'll list its pages:\n\n```sh\nvos projects pages NEOC --all\n```"],
      { kind: 'tool', name: 'vos:projects__list_pages', data: JSON.stringify({ kind: 'fetch', status: 'in_progress', input: '{"project":"NEOC"}' }), done: false }
    ]),
    busy: true
  }

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
    args: { conversation: TALK, draft: '', ondraft, onsend, onstop, onanswer, onremove, onback, onlink },
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

{#snippet phone(args: Args)}
  <div style="width:390px;height:844px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ChatThread {...args} />
  </div>
{/snippet}

<Story
  name="A chat"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-title')).toHaveTextContent('What is 12 times 7?')
    await expect(canvas.getByTestId('agents-thread')).toHaveTextContent('12 times 7 is 84.')
    await expect(canvas.getByTestId('agents-composer-send')).toBeVisible()
    // an agent without a picture: its initials beside its name, in the header only (the chat is with it)
    await expect(canvas.getAllByRole('img', { name: 'Scripted agent' })).toHaveLength(1)
    await expect(canvas.getByTestId('agents-avatar-thread').querySelector('[role="img"]')).toHaveAttribute('data-initials', 'SA')
  }}
/>

<!-- A chat with an agent that has a picture: it is beside the agent's name. -->
<Story
  name="An agent with a picture"
  args={{ conversation: { ...TALK, chat: { ...TALK.chat, agent: PICTURED.name, agent_id: PICTURED.id } }, avatar: PICTURE }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-avatar-thread').querySelector('img')).toHaveAttribute('src', PICTURE)
  }}
/>

<!-- Just started: nothing said yet. -->
<Story name="Nothing said yet" args={{ conversation: conversation('n', 'New chat', []) }} />

<!-- The agent is answering: its message so far, the line over the composer says it is thinking, and the button stops it. -->
<Story
  name="An answer on its way"
  args={{ conversation: answering('84 divided by') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-composer-stop')).toBeVisible()
    await expect(canvas.getByTestId('agents-status')).toHaveTextContent('Thinking…')
    await expect(canvas.getByTestId('agents-message-assistant-9')).toHaveTextContent('84 divided by')
  }}
/>

<!-- What an agent did: its thought a quiet line, its two tools one row closed until asked for, its plan a checklist, notes on the chat, and a turn that failed. -->
<Story
  name="What the agent did"
  args={{ conversation: WORK }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-item-note-1')).toHaveTextContent('started a new one')
    await expect(canvas.getByTestId('agents-item-plan-4')).toHaveTextContent('Add milk')
    await expect(canvas.getByTestId('agents-steps-5-summary')).toHaveTextContent('Edited 1 file · Searched once')
    await expect(canvas.getByTestId('agents-steps-5-summary')).toHaveTextContent('· 1 failed')
    await userEvent.click(canvas.getByTestId('agents-steps-5-summary'))
    await expect(canvas.getByTestId('agents-item-tool-5')).toHaveTextContent('Add milk to list.md')
    await userEvent.click(canvas.getByTestId('agents-item-tool-5-summary'))
    await expect(canvas.getByTestId('agents-item-5-diff-0')).toHaveTextContent('+Milk')
    await expect(canvas.getByTestId('agents-item-tool-6-summary')).toHaveTextContent('· failed')
    await expect(canvas.getByTestId('agents-item-reasoning-3-summary')).toHaveTextContent('Thought')
    // no turn runs: nothing over the composer
    await expect(canvas.getByTestId('agents-status').textContent?.trim()).toBe('')
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
    // the turn waits for the person: the question says so, not the line over the composer
    await expect(canvas.getByTestId('agents-status').textContent?.trim()).toBe('')
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

<!-- On a phone, at work: two tool steps as one row, opened, and the first of them opened; a reply in Markdown; and the line over the composer naming the tool still running. -->
<Story
  name="At work, on a phone"
  args={{ conversation: BUSY }}
  template={phone}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('agents-steps-2-summary')).toHaveTextContent('Searched once · Used 1 tool')
    await userEvent.click(canvas.getByTestId('agents-steps-2-summary'))
    await userEvent.click(canvas.getByTestId('agents-item-tool-2-summary'))
    await expect(canvas.getByTestId('agents-item-tool-2')).toHaveTextContent('"query": "vos projects"')
    await expect(canvas.getByTestId('agents-message-assistant-4').querySelector('strong')).toHaveTextContent('NEOC')
    await expect(canvas.getByTestId('agents-status')).toHaveTextContent('Thinking… · vos:projects__list_pages')
    await expect(canvas.getByTestId('agents-composer-stop')).toBeVisible()
  }}
/>
