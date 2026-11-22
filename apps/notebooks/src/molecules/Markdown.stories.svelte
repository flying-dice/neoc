<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import Markdown from './Markdown.svelte'

  interface Args {
    body: string
    testid: string
    resolves?: (title: string) => boolean
    onfollow: (title: string) => void
    onlink: (url: string) => void
    ontag: (tag: string) => void
    ontick: (line: number) => void
  }

  const onfollow = fn<(title: string) => void>()
  const onlink = fn<(url: string) => void>()
  const ontag = fn<(tag: string) => void>()
  const ontick = fn<(line: number) => void>()

  const EVERYTHING = [
    '# Garden plan',
    '',
    'The beds for this year, with **what** goes in *each*. ~~Roses~~ are out; ==tomatoes== are in.',
    'See [[Tomatoes]], [[Herbs|the herb bed]] and [[Cold frame]], or the [seed catalogue](https://example.com/seeds).',
    '',
    '## To do',
    '',
    '- [x] Order the seeds',
    '- [ ] Clear the north bed',
    '  - Mind the `rhubarb`',
    '  - [ ] Move the compost',
    '',
    '1. Dig',
    '2. Sow',
    '',
    '> Sow after the last frost.',
    '> Not before.',
    '',
    '```',
    'bed  width  length',
    'A    1.2    4.0',
    '```',
    '',
    '---',
    '',
    '#garden #plans'
  ].join('\n')

  /** A note as it reads. It keeps nothing: the Markdown comes in, and what was pressed goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/Markdown',
    component: Markdown,
    tags: ['autodocs'],
    args: { body: EVERYTHING, testid: 'reading', resolves: (title) => title !== 'Cold frame', onfollow, onlink, ontag, ontick },
    argTypes: { resolves: { control: false } },
    beforeEach: () => {
      for (const mock of [onfollow, onlink, ontag, ontick]) mock.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;padding:16px;background:var(--bg);color:var(--label)">
    <Markdown {...args} />
  </div>
{/snippet}

<!-- Everything a note can say: headings, styles, links, lists with tasks, a quote, code, a rule and tags. -->
<Story
  name="Everything"
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('heading', { level: 1, name: 'Garden plan' })).toBeVisible()
    await expect(canvas.getByTestId('reading-link-Herbs')).toHaveTextContent('the herb bed')
    await expect(canvas.getByTestId('reading-task-7')).toBeChecked()
    await expect(canvas.getByTestId('reading-task-8')).not.toBeChecked()
    // a link to a note that is not written yet says so
    await expect(canvas.getByTestId('reading-link-Cold frame')).toHaveAttribute('title', 'Cold frame is not written yet')
  }}
/>

<!-- What a note says is never read as markup. -->
<Story
  name="Markup is text"
  args={{ body: 'A note with <b>markup</b> and <img src=x onerror=alert(1)> in it.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('reading')).toHaveTextContent('<b>markup</b>')
    await expect(canvas.getByTestId('reading').querySelector('img, b')).toBeNull()
  }}
/>

<Story
  name="Long words and long lines"
  args={{ body: `# ${'Supercalifragilistic'.repeat(4)}\n\n${'A long line of a note that goes on. '.repeat(12)}\n\n- ${'x'.repeat(120)}` }}
/>

<Story
  name="Pressing"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('reading-link-Tomatoes'))
    await expect(onfollow).toHaveBeenCalledWith('Tomatoes')
    await userEvent.click(canvas.getByTestId('reading-out'))
    await expect(onlink).toHaveBeenCalledWith('https://example.com/seeds')
    await userEvent.click(canvas.getByTestId('reading-tag-garden'))
    await expect(ontag).toHaveBeenCalledWith('garden')
    await userEvent.click(canvas.getByTestId('reading-task-8'))
    await expect(ontick).toHaveBeenCalledWith(8)
    await userEvent.click(canvas.getByTestId('reading-task-10'))
    await expect(ontick).toHaveBeenCalledWith(10)
  }}
/>
