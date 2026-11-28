<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Item } from '../types'
  import MessageBubble from './MessageBubble.svelte'

  const item = (kind: 'user' | 'assistant', text: string, done = true): Item => ({ id: 1, chat: 'c', kind, text, name: '', data: '', done })
  const onlink = fn<(url: string) => void>()

  /** One thing said in a chat. The person's is in a bubble on the right, the agent's across the column, as Markdown. */
  const { Story } = defineMeta({
    title: 'Molecules/MessageBubble',
    component: MessageBubble,
    tags: ['autodocs'],
    args: { item: item('user', 'What is 12 times 7?'), onlink },
    beforeEach: () => {
      onlink.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: { item: Item; onlink: (url: string) => void })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <MessageBubble {...args} testid="chat" />
  </div>
{/snippet}

<Story
  name="From the person"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('chat-message-user-1')).toHaveTextContent('What is 12 times 7?')
  }}
/>

<!-- The person's own line breaks are kept, and their text is never read as Markdown. -->
<Story
  name="From the person, on two lines"
  args={{ item: item('user', 'Vos projects\n**not bold**') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('chat-message-user-1').querySelector('strong')).toBeNull()
  }}
/>

<!-- The agent's: no bubble, its Markdown drawn. -->
<Story
  name="From the agent"
  args={{ item: item('assistant', "Yes. There's one VOS project.\n\n- **NEOC** (key `NEOC`)\n  - `NEOC-1`: \"Make a page\". Task, medium priority, done.\n\nDo you want its pages listed?") }}
  play={async ({ canvas }) => {
    const message = canvas.getByTestId('chat-message-assistant-1')
    await expect(message.querySelector('strong')).toHaveTextContent('NEOC')
    await expect(message.querySelectorAll('li')).toHaveLength(2)
    await expect(message).not.toHaveTextContent('**')
  }}
/>

<!-- The answer is still coming: what has arrived so far, drawn (the line over the composer says the agent is at work). -->
<Story name="Still coming" args={{ item: item('assistant', '12 times 7 is **8', false) }} />

<!-- A long answer wraps in the column; a long word breaks rather than overflow. -->
<Story
  name="Long"
  args={{
    item: item(
      'assistant',
      'First line, which is long enough to wrap when the column is this narrow.\n\nhttps://example.com/a/very/long/path/that/has/no/spaces/in/it/at/all/and/keeps/going'
    )
  }}
  play={async ({ canvas }) => {
    const bubble = canvas.getByTestId('chat-message-assistant-1')
    await expect(bubble.scrollWidth).toBeLessThanOrEqual(bubble.clientWidth)
  }}
/>

<!-- A link the agent gave: a web address opens outside the app. -->
<Story
  name="A link"
  args={{
    item: {
      ...item('assistant', 'The ACP specification'),
      name: 'link',
      data: JSON.stringify({ type: 'link', uri: 'https://agentclientprotocol.com', name: 'The ACP specification' })
    }
  }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('chat-message-1-link')).toHaveAttribute('href', 'https://agentclientprotocol.com')
    // the host opens it: the frame cannot
    await userEvent.click(canvas.getByTestId('chat-message-1-link'))
    await expect(onlink).toHaveBeenCalledWith('https://agentclientprotocol.com')
  }}
/>

<!-- A file on the agent's machine: named, not opened. -->
<Story
  name="A file of the agent's"
  args={{
    item: { ...item('assistant', 'list.md'), name: 'link', data: JSON.stringify({ type: 'link', uri: 'file:///home/ada/project/list.md', name: 'list.md' }) }
  }}
/>

<!-- An image the agent gave. -->
<Story
  name="An image"
  args={{
    item: {
      ...item('assistant', ''),
      name: 'image',
      data: JSON.stringify({
        type: 'image',
        mime_type: 'image/svg+xml',
        data: 'PHN2ZyB4bWxucz0iaHR0cDovL3d3dy53My5vcmcvMjAwMC9zdmciIHdpZHRoPSI4MCIgaGVpZ2h0PSI0MCI+PHJlY3Qgd2lkdGg9IjgwIiBoZWlnaHQ9IjQwIiBmaWxsPSIjZjkwIi8+PC9zdmc+'
      })
    }
  }}
/>
