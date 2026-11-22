<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import type { Message } from 'app:rpc'
  import MessageBubble from './MessageBubble.svelte'

  const message = (role: Message['role'], text: string, done = true): Message => ({ id: 1, chat: 'c', role, text, done })

  /** One thing said in a chat. The person's is on the right in the accent colour, the agent's on the left. */
  const { Story } = defineMeta({
    title: 'Molecules/MessageBubble',
    component: MessageBubble,
    tags: ['autodocs'],
    args: { message: message('user', 'What is 12 times 7?') },
    render: template
  })
</script>

{#snippet template(args: { message: Message })}
  <div style="display:flex;flex-direction:column;width:420px;padding:16px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <MessageBubble {...args} />
  </div>
{/snippet}

<Story
  name="From the person"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-message-user-1')).toHaveTextContent('What is 12 times 7?')
  }}
/>

<Story name="From the agent" args={{ message: message('agent', '12 times 7 is 84.') }} />

<!-- The answer is still coming: what has arrived, and a spinner after it. -->
<Story
  name="Still coming"
  args={{ message: message('agent', '12 times 7', false) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('agents-message-agent-1').querySelector('ui-spinner')).not.toBeNull()
  }}
/>

<!-- Nothing has arrived yet: the spinner alone. -->
<Story name="Nothing yet" args={{ message: message('agent', '', false) }} />

<!-- A long answer wraps inside the bubble and keeps the agent's own line breaks; a long word breaks rather than overflow. -->
<Story
  name="Long"
  args={{
    message: message(
      'agent',
      'First line.\nSecond line, which is long enough to wrap inside the bubble when the column is this narrow.\nhttps://example.com/a/very/long/path/that/has/no/spaces/in/it/at/all/and/keeps/going'
    )
  }}
  play={async ({ canvas }) => {
    const bubble = canvas.getByTestId('agents-message-agent-1')
    await expect(bubble.scrollWidth).toBeLessThanOrEqual(bubble.clientWidth)
  }}
/>
