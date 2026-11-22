<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn, waitFor } from 'storybook/test'
  import TerminalScreen from './TerminalScreen.svelte'
  import type { Screen } from '../lib/terminal.svelte'

  interface Args {
    /** What the story draws once the emulator is there. */
    text: string
    interactive: boolean
    ondata: (text: string) => void
    onresize: (rows: number, cols: number) => void
  }

  const ondata = fn<(text: string) => void>()
  const onresize = fn<(rows: number, cols: number) => void>()
  const encoder = new TextEncoder()

  /** The emulator alone: it draws what it is given and says what was typed. No session, no backend. */
  const { Story } = defineMeta({
    title: 'Organisms/TerminalScreen',
    component: TerminalScreen,
    tags: ['autodocs'],
    args: { text: '$ echo hello\r\nhello\r\n$ ', interactive: true, ondata, onresize },
    beforeEach: () => {
      ondata.mockClear()
      onresize.mockClear()
    },
    render: template
  })

  const screen = (canvas: { getByTestId(id: string): HTMLElement }): HTMLElement => canvas.getByTestId('terminal-screen')
</script>

{#snippet template(args: Args)}
  <div style="width:480px;height:240px">
    <TerminalScreen
      interactive={args.interactive}
      onready={(drawn: Screen) => drawn.write(encoder.encode(args.text), () => {})}
      ondata={args.ondata}
      onbinary={() => {}}
      onresize={args.onresize}
    />
  </div>
{/snippet}

<Story
  name="Output"
  play={async ({ canvas, userEvent }) => {
    await waitFor(() => expect(screen(canvas)).toHaveTextContent('hello'))
    // the emulator has the keyboard from the start: what is typed comes out as text, a key at a time
    await userEvent.keyboard('ls')
    await waitFor(() => expect(ondata).toHaveBeenCalledTimes(2))
    await expect(ondata).toHaveBeenNthCalledWith(1, 'l')
  }}
/>

<Story
  name="Colours"
  args={{
    text: '\u001b[1mbold\u001b[0m \u001b[31mred\u001b[0m \u001b[32mgreen\u001b[0m \u001b[34mblue\u001b[0m \u001b[7mreversed\u001b[0m\r\n$ '
  }}
  play={async ({ canvas }) => {
    await waitFor(() => expect(screen(canvas)).toHaveTextContent('reversed'))
  }}
/>

<Story
  name="Not taking input"
  args={{ interactive: false }}
  play={async ({ canvas, userEvent }) => {
    await waitFor(() => expect(screen(canvas)).toHaveTextContent('hello'))
    await userEvent.keyboard('ls')
    await expect(ondata).not.toHaveBeenCalled()
  }}
/>
