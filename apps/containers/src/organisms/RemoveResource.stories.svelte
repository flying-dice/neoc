<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import RemoveResource from './RemoveResource.svelte'
  import type { Dropping } from '../lib/resources.svelte'

  interface Args {
    removal: Dropping
    onremove: () => void
    oncancel: () => void
  }

  const onremove = fn<() => void>()
  const oncancel = fn<() => void>()

  const asked = (kind: Dropping['kind'], name: string, about: string, users: string[] = [], more: Partial<Dropping> = {}): Dropping => ({
    kind,
    key: name,
    name,
    about,
    users,
    force: false,
    failure: null,
    busy: false,
    ...more
  })
  const REDIS = asked('image', 'redis:7', '3f2a9c11d0e4 · 117 MB · built 14 Nov 2023')
  const ALPINE = asked('image', 'alpine:3.20', '9b1c2d3e4f50 · 64.0 MB · built 2 Mar 2024', ['scratch'])
  const DATA = asked('volume', 'pihole-data', 'local · /var/lib/docker/volumes/pihole-data/_data', ['pihole', 'pihole-copy'])
  const CACHE = asked('volume', 'cache', 'local · /var/lib/docker/volumes/cache/_data')
  const BACKEND = asked('network', 'backend', 'bridge · 172.30.0.0/16', ['api'])
  const RUNNING = 'api runs on the network backend: Docker removes a network only once no running container is on it.'

  /** The question before an image, a volume or a network is removed. What is chosen goes into the question it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/RemoveResource',
    component: RemoveResource,
    tags: ['autodocs'],
    args: { removal: REDIS, onremove, oncancel },
    beforeEach: () => {
      onremove.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:520px;color:var(--label);font:var(--type-body)">
    <RemoveResource {...args} />
  </div>
{/snippet}

<!-- An image no container was made from: removed when asked. -->
<Story
  name="An unused image"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.queryByTestId('containers-remove-resource-force')).toBeNull()
    await userEvent.click(canvas.getByTestId('containers-remove-resource-confirm'))
    await expect(onremove).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-remove-resource-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<!-- An image a container was made from: the question names it, and the button waits for force. -->
<Story
  name="An image in use"
  args={{ removal: { ...ALPINE } }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('containers-remove-resource-about')).toHaveProperty('footer', expect.stringContaining('Made from it: scratch.'))
    await expect(canvas.getByTestId('containers-remove-resource-confirm')).toHaveProperty('disabled', true)
    await userEvent.click(canvas.getByTestId('containers-remove-resource-force'))
    await expect(canvas.getByTestId('containers-remove-resource-confirm')).toHaveProperty('disabled', false)
  }}
/>

<!-- A volume no container uses: its data goes with it. -->
<Story
  name="An unused volume"
  args={{ removal: CACHE }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-resource-about')).toHaveProperty('footer', 'The data in it is deleted with it. This cannot be undone.')
    await expect(canvas.getByTestId('containers-remove-resource-confirm')).toHaveProperty('heading', 'Delete cache and its data')
  }}
/>

<!-- A volume containers use: Docker will not remove it, and the question says who uses it. -->
<Story
  name="A volume in use"
  args={{ removal: DATA }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-resource-about')).toHaveProperty(
      'footer',
      'pihole and pihole-copy use it: Docker removes a volume only once no container uses it, running or not.'
    )
    await expect(canvas.getByTestId('containers-remove-resource-confirm')).toHaveProperty('disabled', true)
  }}
/>

<!-- A network, refused because a running container is on it: the reason, under the button. -->
<Story
  name="A network that is refused"
  args={{ removal: { ...BACKEND, failure: RUNNING } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-resource-failure')).toHaveProperty('footer', RUNNING)
  }}
/>

<Story
  name="Removing"
  args={{ removal: { ...CACHE, busy: true } }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-resource-busy')).toBeVisible()
    await expect(canvas.getByTestId('containers-remove-resource-confirm')).toHaveProperty('disabled', true)
  }}
/>
