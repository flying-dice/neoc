<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  type Container = VosApp.Containers.Container
  type Stack = VosApp.Containers.Stack
  import ContainerList from './ContainerList.svelte'
  import type { Action } from '../lib/containers.svelte'
  import { CONTAINERS, stacksOf } from '../lib/fakeApi'

  interface Args {
    title: string
    containers: readonly Container[]
    stack?: Stack
    stackBusy?: boolean
    busy: (container: Container) => boolean
    back?: boolean
    onopen: (container: Container) => void
    ontoggle: (container: Container) => void
    onstack?: (stack: Stack, action: Action) => void
    onback?: () => void
  }

  const onopen = fn<(container: Container) => void>()
  const ontoggle = fn<(container: Container) => void>()
  const onstack = fn<(stack: Stack, action: Action) => void>()
  const onback = fn<() => void>()
  const heard = [onopen, ontoggle, onstack, onback]

  const of = (stack: string): Container[] => CONTAINERS.filter((c) => c.stack === stack)
  const [BLOG, MEDIA] = stacksOf(CONTAINERS) as [Stack, Stack]
  const idle = (): boolean => false

  /** The containers that are shown. It keeps only the question it has asked before a stack is removed. */
  const { Story } = defineMeta({
    title: 'Organisms/ContainerList',
    component: ContainerList,
    tags: ['autodocs'],
    args: { title: 'All Containers', containers: CONTAINERS, busy: idle, onopen, ontoggle, onstack, onback },
    argTypes: { busy: { control: false } },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ContainerList {...args} />
  </div>
{/snippet}

<!-- Every container: nothing acts on all of them at once. -->
<Story
  name="All containers"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-title')).toHaveTextContent('All Containers')
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('4 of 6 containers running')
    await expect(canvas.queryByTestId('containers-stack-actions')).toBeNull()
    await expect(canvas.queryByTestId('containers-back')).toBeNull()
  }}
/>

<!-- A stack that is half up: it can be started, stopped, restarted and removed, and says where it came from. -->
<Story
  name="A stack"
  args={{ title: 'media', containers: of('media'), stack: MEDIA }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('1 of 2 containers running · /srv/media')
    await expect(canvas.getByTestId('containers-stack-start')).not.toHaveAttribute('disabled')
    await expect(canvas.getByTestId('containers-stack-stop')).not.toHaveAttribute('disabled')
  }}
/>

<!-- All of it runs: there is nothing left to start. -->
<Story
  name="A stack that is up"
  args={{ title: 'blog', containers: of('blog'), stack: BLOG }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-stack-start')).toHaveAttribute('disabled')
    await expect(canvas.getByTestId('containers-stack-stop')).not.toHaveAttribute('disabled')
  }}
/>

<!-- A call is in flight for the stack: its buttons wait, and so does each of its containers. -->
<Story
  name="A stack being worked on"
  args={{ title: 'blog', containers: of('blog'), stack: BLOG, stackBusy: true, busy: () => true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-stack-busy')).toBeVisible()
    await expect(canvas.getByTestId('containers-stack-restart')).toHaveAttribute('disabled')
    await expect(canvas.getByTestId('containers-item-blog-db-1-busy')).toBeVisible()
  }}
/>

<!-- Removing a stack removes its containers: it is asked about first. -->
<Story
  name="Asked before a stack is removed"
  args={{ title: 'blog', containers: of('blog'), stack: BLOG }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-stack-remove'))
    await expect(alertDialog(canvas.getByTestId('containers-stack-confirm'))).toHaveTextContent('Remove blog?')
    await expect(onstack).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Empty"
  args={{ containers: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-empty')).toBeVisible()
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('No containers')
  }}
/>

<!-- A window too narrow for the stacks beside it: the way back to them. -->
<Story
  name="With the way back"
  args={{ back: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-back')).toBeVisible()
  }}
/>

<Story
  name="Acting"
  tags={['play-only']}
  args={{ title: 'media', containers: of('media'), stack: MEDIA, back: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-item-media-jellyfin-1-stop'))
    await expect(ontoggle).toHaveBeenLastCalledWith(of('media')[0])
    await userEvent.click(canvas.getByTestId('containers-item-media-transcoder-1-open'))
    await expect(onopen).toHaveBeenLastCalledWith(of('media')[1])
    await userEvent.click(canvas.getByTestId('containers-stack-restart'))
    await expect(onstack).toHaveBeenLastCalledWith(MEDIA, 'restart')
    // removing waits for the answer; cancelling asks for nothing
    await userEvent.click(canvas.getByTestId('containers-stack-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-stack-confirm'), 'cancel'))
    await expect(canvas.queryByTestId('containers-stack-confirm')).toBeNull()
    await expect(onstack).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-stack-remove'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-stack-confirm'), 'action'))
    await expect(onstack).toHaveBeenLastCalledWith(MEDIA, 'remove')
    await userEvent.click(canvas.getByTestId('containers-back'))
    await expect(onback).toHaveBeenCalledTimes(1)
  }}
/>
