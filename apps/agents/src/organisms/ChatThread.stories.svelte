<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Conversation } from 'app:rpc'
  import ChatThread from './ChatThread.svelte'
  import { conversation } from '../lib/fakeApi'

  interface Args {
    conversation: Conversation
    draft: string
    failure?: string | null
    back?: boolean
    ondraft: (text: string) => void
    onsend: () => void
    onstop: () => void
    onback?: () => void
  }

  const ondraft = fn<(text: string) => void>()
  const onsend = fn<() => void>()
  const onstop = fn<() => void>()
  const onback = fn<() => void>()

  const TALK = conversation('a', 'What is 12 times 7?', [
    ['user', 'What is 12 times 7?'],
    ['agent', '12 times 7 is 84.'],
    ['user', 'And divided by 4?']
  ])
  const answering = (text: string): Conversation => ({
    ...TALK,
    messages: [...TALK.messages, { id: 9, chat: 'a', role: 'agent', text, done: false }],
    busy: true
  })
  const long = conversation(
    'l',
    'A long chat',
    Array.from({ length: 24 }, (_, i): ['user' | 'agent', string] => [i % 2 === 0 ? 'user' : 'agent', `Line ${i + 1}`])
  )

  /** One chat: its title, what was said, the composer. It keeps nothing but where the log is scrolled to. */
  const { Story } = defineMeta({
    title: 'Organisms/ChatThread',
    component: ChatThread,
    tags: ['autodocs'],
    args: { conversation: TALK, draft: '', ondraft, onsend, onstop, onback },
    beforeEach: () => {
      onback.mockClear()
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
    await expect(canvas.getByTestId('agents-message-agent-9')).toHaveTextContent('84 divided by')
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

<!-- A window too narrow for the list as well: the way back to it. -->
<Story
  name="With the way back"
  args={{ back: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('agents-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
