<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, userEvent, waitFor, within } from 'storybook/test'
  import Markdown from './Markdown.svelte'

  const { Story } = defineMeta({
    title: 'Atoms/Markdown',
    component: Markdown,
    tags: ['autodocs'],
    args: { testid: 'markdown', text: '', selectable: false },
    parameters: { layout: 'padded' }
  })

  const sample = [
    '# Release notes',
    '',
    'AgentOS **1.0** adds _tables_, `inline code` and ~~old~~ new [links](https://example.com/docs).',
    '',
    '## Checklist',
    '',
    '- [x] Write the parser',
    '- [ ] Ship it',
    '- plain item with a second line',
    '  that wraps',
    '',
    '1. First',
    '2. Second',
    '',
    '> A quote from an agent:',
    '> keep the text, drop the markup.',
    '',
    '```ts',
    'const greet = (name: string) => `hello ${name}`',
    '```',
    '',
    '| Tool | Calls | Notes |',
    '|:-----|------:|-------|',
    '| read | 12 | fast |',
    '| write | 3 | careful |',
    '',
    '---',
    '',
    'Last paragraph.'
  ].join('\n')

  const hostile = [
    '<scr' + 'ipt>alert("pwned")</scr' + 'ipt>',
    '',
    '<img src=x onerror=alert(1)>',
    '',
    'Inline <b onclick="steal()">HTML</b> stays text.',
    '',
    '[run this](javascript:alert(1)) and [this is fine](https://example.com)',
    '',
    '![tracking pixel](https://example.com/p.gif)',
    '',
    '`<iframe src=//evil></iframe>`'
  ].join('\n')

  const open = fn()
  const aux = fn()
</script>

<Story name="Document" args={{ text: sample, selectable: true }} />

<Story name="Untrusted source" args={{ text: hostile }} />

<Story
  name="Links open through the host"
  args={{ text: 'Read the [docs](https://example.com/docs) or [mail us](mailto:hi@example.com).', onlink: open }}
  play={async ({ canvasElement }) => {
    const canvas = within(canvasElement)
    await userEvent.click(canvas.getByTestId('markdown-link-0'))
    await waitFor(() => expect(open).toHaveBeenCalledWith('https://example.com/docs'))
    await userEvent.click(canvas.getByTestId('markdown-link-1'))
    await waitFor(() => expect(open).toHaveBeenCalledWith('mailto:hi@example.com'))
  }}
/>

<!-- A middle click would open a tab straight from the href: it asks the host like a click does, and the browser does nothing itself. -->
<Story
  name="Middle click asks too"
  args={{ text: 'Read the [docs](https://example.com/docs).', onlink: aux }}
  play={async ({ canvasElement }) => {
    const link = within(canvasElement).getByTestId('markdown-link-0')
    const middle = new MouseEvent('auxclick', { button: 1, bubbles: true, cancelable: true })
    link.dispatchEvent(middle)
    await expect(middle.defaultPrevented).toBe(true)
    await waitFor(() => expect(aux).toHaveBeenCalledWith('https://example.com/docs'))
    // the context menu's button (2) is not ours
    const right = new MouseEvent('auxclick', { button: 2, bubbles: true, cancelable: true })
    link.dispatchEvent(right)
    await expect(right.defaultPrevented).toBe(false)
    await expect(aux).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="No script runs"
  args={{ text: hostile }}
  play={async ({ canvasElement }) => {
    const root = within(canvasElement).getByTestId('markdown')
    await expect(root.querySelector('script, img, iframe, b, [onclick], [onerror]')).toBeNull()
    await expect(root).toHaveTextContent('<script>alert("pwned")</script>')
    await expect(root).toHaveTextContent('Inline <b onclick="steal()">HTML</b> stays text.')
    await expect(root.querySelectorAll('a')).toHaveLength(1)
    await expect(root.querySelector('a')).toHaveAttribute('href', 'https://example.com')
  }}
/>

<Story name="Only tables" args={{ text: sample, options: { tables: true, tasklist: false, code: false } }} />
