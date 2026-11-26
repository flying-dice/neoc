<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  type Container = VosApp.Containers.Container
  type Details = VosApp.Containers.Details
  type Logs = VosApp.Containers.Logs
  import ContainerInspector from './ContainerInspector.svelte'
  import type { Action, Tab } from '../lib/containers.svelte'
  import { container } from '../lib/fakeApi'

  interface Args {
    container: Container
    busy?: boolean
    tab: Tab
    logs: Logs | null
    details: Details | null
    onact: (action: Action) => void
    ontab: (tab: Tab) => void
    onback: () => void
  }

  const onact = fn<(action: Action) => void>()
  const ontab = fn<(tab: Tab) => void>()
  const onback = fn<() => void>()
  const heard = [onact, ontab, onback]

  const DB = container('blog-db-1', 'postgres:16', 'running', { stack: 'blog', service: 'db', ports: ['5432:5432'] })
  const LINES = Array.from({ length: 40 }, (_, i) => `2026-06-09 09:${String(i).padStart(2, '0')}:00 UTC [1] LOG:  checkpoint ${i + 1} complete`)
  const logs = (lines: readonly string[], truncated = false): Logs => ({ container: DB.id, text: lines.map((l) => `${l}\n`).join(''), truncated })
  const DETAILS: Details = {
    container: DB.id,
    command: 'docker-entrypoint.sh postgres',
    restart: 'unless-stopped',
    health: 'healthy',
    mounts: ['blog_db-data -> /var/lib/postgresql/data'],
    networks: ['blog_default'],
    environment: ['PATH', 'PGDATA', 'POSTGRES_PASSWORD']
  }

  /** One container looked at. It keeps only the question it has asked before the container is removed. */
  const { Story } = defineMeta({
    title: 'Organisms/ContainerInspector',
    component: ContainerInspector,
    tags: ['autodocs'],
    args: { container: DB, tab: 'logs', logs: logs(LINES.slice(0, 6)), details: null, onact, ontab, onback },
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

<!-- What it was made with. The values of its environment are never sent, only the names. -->
<Story
  name="Details"
  args={{ tab: 'details', details: DETAILS }}
  play={async ({ canvas }) => {
    const details = canvas.getByTestId('containers-details')
    await expect(details).toHaveTextContent('docker-entrypoint.sh postgres')
    await expect(details).toHaveTextContent('unless-stopped')
    await expect(details).toHaveTextContent('healthy')
    await expect(details).toHaveTextContent('blog · db')
    await expect(details).toHaveTextContent('POSTGRES_PASSWORD')
  }}
/>

<!-- A stopped container on its own: it can be started, and has no stack or health to show. -->
<Story
  name="Stopped"
  args={{
    container: container('scratch', 'alpine:3.20', 'exited'),
    tab: 'details',
    details: { ...DETAILS, command: '/bin/sh', restart: 'no', health: '', mounts: [], environment: [] }
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-inspector-start')).toBeVisible()
    await expect(canvas.getByTestId('containers-details')).not.toHaveTextContent('Stack')
    await expect(canvas.getByTestId('containers-details')).toHaveTextContent('None published')
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

<!-- Removing is asked about first, and says what it does to a container that runs. -->
<Story
  name="Asked before it is removed"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-inspector-remove'))
    await expect(alertDialog(canvas.getByTestId('containers-inspector-confirm'))).toHaveTextContent('Remove blog-db-1?')
    await expect(alertDialog(canvas.getByTestId('containers-inspector-confirm'))).toHaveTextContent('It is running')
    await expect(onact).not.toHaveBeenCalled()
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
    await userEvent.click(canvas.getByTestId('containers-inspector-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-inspector-confirm'), 'cancel'))
    await expect(onact).toHaveBeenCalledTimes(2)
    await userEvent.click(canvas.getByTestId('containers-inspector-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-inspector-confirm'), 'action'))
    await expect(onact).toHaveBeenLastCalledWith('remove')
    await userEvent.click(canvas.getByTestId('containers-inspector-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
