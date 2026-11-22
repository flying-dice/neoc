<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import SessionBar from './SessionBar.svelte'

  interface Args {
    message: string
    action?: string
    busy?: boolean
    onaction: () => void
  }

  const onaction = fn<() => void>()

  /** The line under the terminal: what happened to the session, and the one thing to do about it. */
  const { Story } = defineMeta({
    title: 'Molecules/SessionBar',
    component: SessionBar,
    tags: ['autodocs'],
    args: { message: 'The shell exited.', action: 'New Session', busy: false, onaction },
    beforeEach: () => onaction.mockClear(),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div data-theme="dark" style="width:420px;background:var(--bg);color:var(--label);color-scheme:dark">
    <SessionBar {...args} />
  </div>
{/snippet}

<Story
  name="The shell exited"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('terminal-bar-message')).toHaveTextContent('The shell exited.')
    await userEvent.click(canvas.getByTestId('terminal-bar-action'))
    await expect(onaction).toHaveBeenCalledTimes(1)
  }}
/>

<Story
  name="Starting"
  args={{ message: 'Starting the shell…', action: undefined, busy: true }}
  play={async ({ canvas }) => {
    await expect(canvas.queryByTestId('terminal-bar-action')).toBeNull()
  }}
/>

<Story
  name="A long reason"
  args={{ message: 'The host is not answering. What you type is sent when the connection is back.', action: undefined }}
/>
