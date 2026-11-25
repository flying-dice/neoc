<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Engine, Stack } from '@containers/rpc'
  import StackSidebar from './StackSidebar.svelte'
  import { ALL, stackScope, type Scope } from '../lib/containers.svelte'
  import { CONTAINERS, stacksOf } from '../lib/fakeApi'

  interface Args {
    engine: Engine
    total: number
    stacks: readonly Stack[]
    selected?: Scope
    draft: string
    onselect: (scope: Scope) => void
    ondraft: (address: string) => void
    onconnect: () => void
  }

  const onselect = fn<(scope: Scope) => void>()
  const ondraft = fn<(address: string) => void>()
  const onconnect = fn<() => void>()

  const HERE: Engine = { reachable: true, version: '27.3.1', host: '', problem: '' }
  const STACKS = stacksOf(CONTAINERS)

  /** What there is to show, and which Docker this is. It keeps only whether the address field is out. */
  const { Story } = defineMeta({
    title: 'Organisms/StackSidebar',
    component: StackSidebar,
    tags: ['autodocs'],
    args: { engine: HERE, total: CONTAINERS.length, stacks: STACKS, selected: ALL, draft: '', onselect, ondraft, onconnect },
    beforeEach: () => {
      onselect.mockClear()
      ondraft.mockClear()
      onconnect.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:280px;height:460px;color:var(--label);font:var(--type-body)">
    <StackSidebar {...args} />
  </div>
{/snippet}

<!-- Every container is shown; one stack is all up, the other half. -->
<Story
  name="Stacks"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-scope-all')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('containers-scope-all')).toHaveProperty('value', '6')
    await expect(canvas.getByTestId('containers-stack-blog')).toHaveProperty('subheading', 'All 2 running')
    await expect(canvas.getByTestId('containers-stack-media')).toHaveProperty('subheading', '1 of 2 running')
    await expect(canvas.getByTestId('containers-engine')).toHaveTextContent('Docker 27.3.1')
    await expect(canvas.getByTestId('containers-engine')).toHaveTextContent('This machine')
  }}
/>

<Story
  name="A stack shown"
  args={{ selected: stackScope('media') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-stack-media')).toHaveAttribute('selected', '')
    await expect(canvas.getByTestId('containers-scope-all')).not.toHaveAttribute('selected')
  }}
/>

<!-- Containers, none of them Compose's: the sidebar says where a stack would come from. -->
<Story
  name="No stacks"
  args={{ stacks: [], total: 2 }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-no-stacks')).toBeVisible()
    await expect(canvas.queryByTestId('containers-stacks')).toBeNull()
  }}
/>

<!-- Docker cannot be reached: nothing to show, and the address it was looked for at. -->
<Story
  name="Docker away"
  args={{ engine: { reachable: false, version: '', host: 'tcp://nas.local:2375', problem: 'Docker cannot be reached at tcp://nas.local:2375.' }, total: 0, stacks: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-engine')).toHaveTextContent('tcp://nas.local:2375')
    await expect(canvas.queryByTestId('containers-no-stacks')).toBeNull()
  }}
/>

<!-- The gear brings the address field out. -->
<Story
  name="Choosing another Docker"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.queryByTestId('containers-host-input')).toBeNull()
    await userEvent.click(canvas.getByTestId('containers-choose'))
    await expect(canvas.getByTestId('containers-host-input')).toBeVisible()
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-stack-blog'))
    await expect(onselect).toHaveBeenLastCalledWith(stackScope('blog'))
    await userEvent.click(canvas.getByTestId('containers-scope-all'))
    await expect(onselect).toHaveBeenLastCalledWith(ALL)
    await userEvent.click(canvas.getByTestId('containers-choose'))
    await userEvent.click(canvas.getByTestId('containers-host-connect'))
    await expect(onconnect).toHaveBeenCalledTimes(1)
  }}
/>
