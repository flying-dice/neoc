<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Container = VosApp.Containers.Container
  type Details = VosApp.Containers.Details
  type Logs = VosApp.Containers.Logs
  import ContainerInspector from './ContainerInspector.svelte'
  import type { Action, Tab } from '../lib/containers.svelte'
  import { container, specOf } from '../lib/fakeApi'

  interface Args {
    container: Container
    busy?: boolean
    tab: Tab
    logs: Logs | null
    details: Details | null
    onact: (action: Exclude<Action, 'remove'>) => void
    ontab: (tab: Tab) => void
    onback: () => void
    onedit: () => void
    onduplicate: () => void
    onremove: () => void
  }

  const onact = fn<(action: Exclude<Action, 'remove'>) => void>()
  const ontab = fn<(tab: Tab) => void>()
  const onback = fn<() => void>()
  const onedit = fn<() => void>()
  const onduplicate = fn<() => void>()
  const onremove = fn<() => void>()
  const heard = [onact, ontab, onback, onedit, onduplicate, onremove]

  const DB = container('blog-db-1', 'postgres:16', 'running', { stack: 'blog', service: 'db', ports: ['5432:5432'] })
  const LINES = Array.from({ length: 40 }, (_, i) => `2026-06-09 09:${String(i).padStart(2, '0')}:00 UTC [1] LOG:  checkpoint ${i + 1} complete`)
  const logs = (lines: readonly string[], truncated = false): Logs => ({ container: DB.id, text: lines.map((l) => `${l}\n`).join(''), truncated })
  const DETAILS: Details = {
    container: DB.id,
    command: 'docker-entrypoint.sh postgres',
    health: 'healthy',
    mounts: ['blog_db-data -> /var/lib/postgresql/data', '/srv/blog/init -> /docker-entrypoint-initdb.d (read-only)'],
    networks: ['blog_default'],
    spec: {
      ...specOf(DB),
      ports: [{ host_ip: '127.0.0.1', host_port: '5432', container_port: '5432', protocol: 'tcp' }],
      restart: 'on-failure',
      retries: '5',
      working_dir: '/var/lib/postgresql',
      user: 'postgres'
    }
  }

  /** One container looked at. It keeps only the question it has asked before the container is removed. */
  const { Story } = defineMeta({
    title: 'Organisms/ContainerInspector',
    component: ContainerInspector,
    tags: ['autodocs'],
    args: { container: DB, tab: 'logs', logs: logs(LINES.slice(0, 6)), details: null, onact, ontab, onback, onedit, onduplicate, onremove },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ContainerInspector {...args} />
  </div>
{/snippet}

<!-- A running container and what it wrote. -->
<Story
  name="Logs"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-inspector-name')).toHaveTextContent('blog-db-1')
    await expect(canvas.getByTestId('containers-inspector-status')).toHaveTextContent('Up 2 hours')
    await expect(canvas.getByTestId('containers-logs')).toHaveTextContent('checkpoint 6 complete')
    await expect(canvas.getByTestId('containers-inspector-stop')).toBeVisible()
    await expect(canvas.queryByTestId('containers-logs-truncated')).toBeNull()
  }}
/>

<!-- More than fits: the newest line is in view, and the logs say they are only the end. -->
<Story
  name="Long logs"
  args={{ logs: logs(LINES, true) }}
  play={async ({ canvas }) => {
    const output = canvas.getByTestId('containers-logs')
    await expect(canvas.getByTestId('containers-logs-truncated')).toBeVisible()
    await expect(output.scrollHeight).toBeGreaterThan(output.clientHeight)
    await expect(output.scrollTop + output.clientHeight).toBeGreaterThanOrEqual(output.scrollHeight - 1)
  }}
/>

<Story
  name="No logs"
  args={{ logs: logs([]) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-no-logs')).toBeVisible()
  }}
/>

<Story name="Reading the logs" args={{ logs: null }} />

<!-- What it was made with: its ports, mounts, network, restart policy and labels. The values of its environment are hidden until they are asked for. -->
<Story
  name="Details"
  args={{ tab: 'details', details: DETAILS }}
  play={async ({ canvas, userEvent }) => {
    const details = canvas.getByTestId('containers-details')
    await expect(details).toHaveTextContent('docker-entrypoint.sh postgres')
    await expect(details).toHaveTextContent('127.0.0.1:5432 → 5432/tcp')
    await expect(details).toHaveTextContent('on-failure, at most 5 times')
    await expect(details).toHaveTextContent('healthy')
    await expect(details).toHaveTextContent('blog · db')
    await expect(details).toHaveTextContent('(read-only)')
    await expect(details).toHaveTextContent('com.docker.compose.service=db')
    await expect(details).toHaveTextContent('/var/lib/postgresql')
    await expect(details).toHaveTextContent('POSTGRES_PASSWORD=')
    await expect(details).not.toHaveTextContent('correct-horse')
    await userEvent.click(canvas.getByTestId('containers-details-reveal'))
    await expect(details).toHaveTextContent('POSTGRES_PASSWORD=correct-horse')
    await userEvent.click(canvas.getByTestId('containers-details-reveal'))
    await expect(details).not.toHaveTextContent('correct-horse')
  }}
/>

<!-- A stopped container on its own: it can be started, and has no stack or health to show. -->
<Story
  name="Stopped"
  args={{
    container: container('scratch', 'alpine:3.20', 'exited'),
    tab: 'details',
    details: {
      ...DETAILS,
      command: '/bin/sh',
      health: '',
      mounts: [],
      spec: { ...specOf(container('scratch', 'alpine:3.20', 'exited')), environment: [], mounts: [] }
    }
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-inspector-start')).toBeVisible()
    await expect(canvas.getByTestId('containers-details')).not.toHaveTextContent('Stack')
    await expect(canvas.getByTestId('containers-details')).toHaveTextContent('None published')
    await expect(canvas.queryByTestId('containers-details-reveal')).toBeNull()
  }}
/>

<Story
  name="Being worked on"
  args={{ busy: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-inspector-busy')).toBeVisible()
    await expect(canvas.getByTestId('containers-inspector-restart')).toHaveAttribute('disabled')
  }}
/>


<Story
  name="Acting"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-inspector-stop'))
    await expect(onact).toHaveBeenLastCalledWith('stop')
    await userEvent.click(canvas.getByTestId('containers-inspector-restart'))
    await expect(onact).toHaveBeenLastCalledWith('restart')
    // the kit's segmented control holds its segments in its shadow tree
    await userEvent.click(canvas.getByTestId('containers-inspector-tab').shadowRoot!.querySelector('[data-testid="containers-inspector-tab-details"]')!)
    await expect(ontab).toHaveBeenLastCalledWith('details')
    await userEvent.click(canvas.getByTestId('containers-inspector-edit'))
    await expect(onedit).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-inspector-duplicate'))
    await expect(onduplicate).toHaveBeenCalledTimes(1)
    // removing asks first: that is the page's
    await userEvent.click(canvas.getByTestId('containers-inspector-remove'))
    await expect(onremove).toHaveBeenCalledTimes(1)
    await expect(onact).toHaveBeenCalledTimes(2)
    await userEvent.click(canvas.getByTestId('containers-inspector-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
