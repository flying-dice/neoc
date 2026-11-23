<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect } from 'storybook/test'
  import type { Logs } from '../types'
  import ContainerLog from './ContainerLog.svelte'

  interface Args {
    absent: boolean
    none: string
    logs: Logs | null
    testid: string
  }

  const text = [
    'dnsmasq: started, version 2.90 cachesize 1000',
    'dnsmasq: using nameserver 1.1.1.1#53',
    'dnsmasq: query[A] nas.home from 192.168.1.7',
    'dnsmasq: config nas.home is 192.168.1.10',
    'dnsmasq: query[A] example.com from 192.168.1.7',
    'dnsmasq: forwarded example.com to 1.1.1.1',
    ''
  ].join('\n')

  /** The end of what a container has written. */
  const { Story } = defineMeta({
    title: 'Organisms/ContainerLog',
    component: ContainerLog,
    tags: ['autodocs'],
    args: { absent: false, none: 'The server has no container. It writes here once it is turned on.', logs: { text, truncated: false }, testid: 'dns' },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:320px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ContainerLog {...args} />
  </div>
{/snippet}

<Story
  name="Questions and answers"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-log')).toHaveTextContent('query[A] nas.home')
  }}
/>

<Story name="The end of a longer log" args={{ logs: { text, truncated: true } }} />
<Story name="Being read" args={{ logs: null }} />
<Story name="Nothing written" args={{ logs: { text: '', truncated: false } }} />

<Story
  name="No container"
  args={{ absent: true, logs: null }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-log-none')).toBeVisible()
  }}
/>
