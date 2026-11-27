<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import RemoveContainer from './RemoveContainer.svelte'
  import type { Removal } from '../lib/containers.svelte'
  import { container } from '../lib/fakeApi'

  interface Args {
    removal: Removal
    onremove: () => void
    oncancel: () => void
  }

  const onremove = fn<() => void>()
  const oncancel = fn<() => void>()

  const asked = (c: Removal['container'], more: Partial<Removal> = {}): Removal => ({ container: c, force: false, volumes: false, failure: null, busy: false, ...more })
  const SCRATCH = container('scratch', 'alpine:3.20', 'exited')
  const PIHOLE = container('pihole', 'pihole/pihole:2024.07.0', 'running')
  const DB = container('blog-db-1', 'postgres:16', 'exited', { stack: 'blog', service: 'db' })
  const RUNNING = 'cannot remove container "/pihole": container is running: stop the container before removing or force remove'

  /** The question before a container is removed. What is chosen goes into the question it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/RemoveContainer',
    component: RemoveContainer,
    tags: ['autodocs'],
    args: { removal: asked(SCRATCH), onremove, oncancel },
    beforeEach: () => {
      onremove.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:520px;height:560px;color:var(--label);font:var(--type-body)">
    <RemoveContainer {...args} />
  </div>
{/snippet}

<!-- A stopped container: removed at once when asked, its anonymous volumes with it only when chosen. -->
<Story
  name="A stopped container"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.queryByTestId('containers-remove-force')).toBeNull()
    await expect(canvas.getByTestId('containers-remove-volumes')).toHaveProperty('checked', false)
    await userEvent.click(canvas.getByTestId('containers-remove-confirm'))
    await expect(onremove).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-remove-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A running container: Docker removes it only when forced, so the button waits for that choice. -->
<Story
  name="A running container"
  args={{ removal: asked(PIHOLE) }}
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('containers-remove-confirm')).toHaveProperty('disabled', true)
    await userEvent.click(canvas.getByTestId('containers-remove-force'))
    await expect(canvas.getByTestId('containers-remove-confirm')).toHaveProperty('disabled', false)
  }}
/>

<!-- A container of a Compose stack: it can be removed, and the question says Compose makes it again. -->
<Story
  name="A container of a stack"
  args={{ removal: asked(DB) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-about')).toHaveProperty('footer', 'It is the db service of the Compose stack blog. Compose makes it again on the next docker compose up.')
  }}
/>

<!-- Docker refused: its reason, under the button. -->
<Story
  name="Refused"
  args={{ removal: asked(PIHOLE, { force: true, failure: RUNNING }) }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-remove-failure')).toHaveProperty('footer', RUNNING)
  }}
/>
