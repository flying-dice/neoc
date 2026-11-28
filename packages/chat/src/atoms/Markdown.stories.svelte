<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Markdown from './Markdown.svelte'

  const onlink = fn<(url: string) => void>()

  const EVERYTHING = [
    '## The VOS projects',
    '',
    "There's one VOS project, and **one** issue in it:",
    '',
    '- **NEOC** (key `NEOC`)',
    '  - `NEOC-1`: "Make a page". Task, *medium* priority, ~~open~~ done.',
    '1. Read the [ACP specification](https://agentclientprotocol.com)',
    '2. Answer',
    '',
    '> A quote from the issue.',
    '',
    '```sh',
    'git -C ~/Projects/neoc log --oneline --decorate --graph --all | head -50 # a line longer than the column',
    '```',
    '',
    '| Key | Title | Status |',
    '| --- | --- | --- |',
    '| NEOC-1 | Make a page | Done |',
    '',
    '- [x] Cloned',
    '- [ ] Built'
  ].join('\n')

  // a script tag written out would end this one: the agent's `<` is made, not written
  const LT = String.fromCharCode(60)
  const SCRIPT = `${LT}script>alert(1)${LT}/script>`
  const TRAP = `Here is ${SCRIPT} and ${LT}b onclick="alert(1)">bold${LT}/b>, and [a trap](javascript:alert(1)).`

  /** What the agent wrote, drawn as Markdown. Its HTML is text, and an address that could run code goes nowhere. */
  const { Story } = defineMeta({
    title: 'Atoms/Markdown',
    component: Markdown,
    tags: ['autodocs'],
    args: { text: EVERYTHING, onlink },
    beforeEach: () => {
      onlink.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: { text: string; onlink: (url: string) => void })}
  <div style="width:390px;padding:16px;box-sizing:border-box;background:var(--bg);color:var(--label);font:var(--type-body)">
    <Markdown {...args} testid="chat-markdown" />
  </div>
{/snippet}

<!-- Headings, emphasis, lists, a link, a quote, a code block that scrolls sideways, a table and a task list. -->
<Story
  name="Everything"
  play={async ({ canvasElement, userEvent }) => {
    await expect(canvasElement.querySelector('li strong')).toHaveTextContent('NEOC')
    await expect(canvasElement.querySelector('h2')).toHaveTextContent('The VOS projects')
    await expect(canvasElement.querySelector('pre')).toHaveAttribute('data-scrolls-x')
    await expect(canvasElement.querySelector('table')).not.toBeNull()
    const link = canvasElement.querySelector('a')!
    await expect(link).toHaveAttribute('rel', 'noopener noreferrer')
    // the host opens it: the frame cannot
    await userEvent.click(link)
    await expect(onlink).toHaveBeenCalledWith('https://agentclientprotocol.com')
  }}
/>

<!-- HTML the agent wrote is shown as text; a javascript: link keeps its words and goes nowhere. -->
<Story
  name="HTML and a dangerous link"
  args={{ text: TRAP }}
  play={async ({ canvasElement, userEvent }) => {
    await expect(canvasElement.querySelector('script')).toBeNull()
    await expect(canvasElement.querySelector('b')).toBeNull()
    await expect(canvasElement).toHaveTextContent(SCRIPT)
    const link = canvasElement.querySelector('a')!
    await expect(link).toHaveAttribute('href', '')
    await userEvent.click(link)
    await expect(onlink).not.toHaveBeenCalled()
  }}
/>

<!-- Still streaming in: a code fence not closed yet runs to the end, inside the column. -->
<Story
  name="A code block still coming"
  args={{ text: 'Run this:\n\n```sh\ncargo test -p neoc-vm-lib-markdown --all-targets -- --nocapture' }}
  play={async ({ canvasElement }) => {
    const pre = canvasElement.querySelector('pre')!
    await expect(pre).toHaveTextContent('cargo test')
    await expect(pre.getBoundingClientRect().right).toBeLessThanOrEqual(canvasElement.querySelector('.markdown')!.getBoundingClientRect().right + 1)
  }}
/>
