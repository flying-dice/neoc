<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import { alertButton, alertDialog } from '@vos/storybook/kit'
  type Volume = VosApp.Containers.Volume
  import VolumeList from './VolumeList.svelte'
  import { volume } from '../lib/fakeApi'

  interface Args {
    volumes: readonly Volume[]
    pruning?: boolean
    report?: string
    back?: boolean
    onback?: () => void
    onremove: (volume: Volume) => void
    onnew: () => void
    onprune: () => void
  }

  const onback = fn<() => void>()
  const onremove = fn<(volume: Volume) => void>()
  const onnew = fn<() => void>()
  const onprune = fn<() => void>()
  const heard = [onback, onremove, onnew, onprune]

  const VOLUMES: Volume[] = [
    { ...volume('blog-db-1-data'), containers: ['blog-db-1'] },
    { ...volume('cache'), containers: [] },
    { ...volume('pihole-data'), containers: ['pihole', 'pihole-copy'] },
    { ...volume('old-uploads'), containers: [] }
  ]

  /** The volumes Docker has. It keeps only the question it asks before a prune. */
  const { Story } = defineMeta({
    title: 'Organisms/VolumeList',
    component: VolumeList,
    tags: ['autodocs'],
    args: { volumes: VOLUMES, onback, onremove, onnew, onprune },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:600px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <VolumeList {...args} />
  </div>
{/snippet}

<!-- Every volume: its driver, where its data is, the day it was made, and the containers that use it. -->
<Story
  name="Volumes"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('4 volumes · 2 unused')
    await expect(canvas.getByTestId('containers-volume-pihole-data-note')).toHaveTextContent('Used by pihole and pihole-copy')
    await expect(canvas.getByTestId('containers-volume-cache')).toHaveTextContent('local · /var/lib/docker/volumes/cache/_data')
    await expect(canvas.getByTestId('containers-volume-cache')).toHaveTextContent('made 27 Nov 2026')
  }}
/>

<Story
  name="No volumes"
  args={{ volumes: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-volumes-empty')).toBeVisible()
    await expect(canvas.getByTestId('containers-volumes-prune')).toHaveProperty('disabled', true)
  }}
/>

<!-- A prune asks first, and says plainly that the data goes with the volumes. -->
<Story
  name="Pruning asks first"
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-volumes-prune'))
    const dialog = alertDialog(canvas.getByTestId('containers-volumes-prune-confirm'))
    await expect(dialog).toHaveTextContent('Delete 2 unused volumes?')
    await expect(dialog).toHaveTextContent('cache and old-uploads are deleted, and all the data in them. This cannot be undone.')
  }}
/>

<Story
  name="Pruned"
  args={{ volumes: VOLUMES.filter((v) => v.containers.length > 0), report: 'Deleted cache and old-uploads with their data, and freed 48.0 MB.' }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-volumes-report')).toHaveTextContent('Deleted cache and old-uploads')
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-volumes-new'))
    await expect(onnew).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-volume-cache-remove'))
    await expect(onremove).toHaveBeenCalledWith(VOLUMES[1])
    await userEvent.click(canvas.getByTestId('containers-volumes-prune'))
    await userEvent.click(alertButton(canvas.getByTestId('containers-volumes-prune-confirm'), 'action'))
    await expect(onprune).toHaveBeenCalledTimes(1)
  }}
/>
