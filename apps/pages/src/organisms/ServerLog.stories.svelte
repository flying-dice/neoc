<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import type { Logs, Server } from 'app:rpc'
  import ServerLog from './ServerLog.svelte'
  import { IMAGE } from '../lib/fakeApi'

  interface Args {
    server: Server
    logs: Logs | null
  }

  const running: Server = { wanted: true, state: 'running', status: 'Up 2 hours', current: true, image: IMAGE, problem: '' }
  const text = [
    'nginx/1.30.0',
    'start worker processes',
    '192.168.1.7 - - [22/Nov/2026:19:52:26 +0000] "GET / HTTP/1.1" 200 2143 "-" "Mozilla/5.0"',
    '192.168.1.7 - - [22/Nov/2026:19:52:26 +0000] "GET /style.css HTTP/1.1" 200 812 "-" "Mozilla/5.0"',
    '192.168.1.9 - - [22/Nov/2026:19:53:02 +0000] "GET /nope HTTP/1.1" 404 146 "-" "curl/8.7.1"',
    ''
  ].join('\n')

  /** The end of what the server has written. */
  const { Story } = defineMeta({
    title: 'Organisms/ServerLog',
    component: ServerLog,
    tags: ['autodocs'],
    args: { server: running, logs: { text, truncated: false } },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:320px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ServerLog {...args} />
  </div>
{/snippet}

<Story
  name="Requests"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-log')).toHaveTextContent('GET /style.css')
  }}
/>

<Story name="The end of a longer log" args={{ logs: { text, truncated: true } }} />
<Story name="Being read" args={{ logs: null }} />
<Story name="Nothing written" args={{ logs: { text: '', truncated: false } }} />

<Story
  name="No site is on"
  args={{ server: { ...running, wanted: false, state: 'absent', status: '', current: false }, logs: null }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-log-none')).toBeVisible()
  }}
/>
