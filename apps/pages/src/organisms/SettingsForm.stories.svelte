<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Engine, Server } from '@pages/rpc'
  import SettingsForm from './SettingsForm.svelte'
  import { IMAGE } from '../lib/fakeApi'

  interface Args {
    port: string
    changed: boolean
    working: boolean
    engine: Engine
    server: Server
    hostDraft: string
    onport: (typed: string) => void
    onsave: () => void
    onreset: () => void
    onhostdraft: (address: string) => void
    onconnect: () => void
  }

  const onsave = fn<() => void>()
  const onreset = fn<() => void>()

  /** The port the named sites share, and which Docker the server runs in. */
  const { Story } = defineMeta({
    title: 'Organisms/SettingsForm',
    component: SettingsForm,
    tags: ['autodocs'],
    args: {
      port: '80',
      changed: false,
      working: false,
      engine: { reachable: true, version: '27.3.1', host: '', problem: '' },
      server: { wanted: true, state: 'running', status: 'Up 2 hours', current: true, image: IMAGE, problem: '' },
      hostDraft: '',
      onport: fn(),
      onsave,
      onreset,
      onhostdraft: fn(),
      onconnect: fn()
    },
    beforeEach: () => {
      onsave.mockClear()
      onreset.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:560px;height:480px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <SettingsForm {...args} />
  </div>
{/snippet}

<!-- As they are kept: there is nothing to save. -->
<Story
  name="As kept"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('pages-engine')).toHaveTextContent('Docker 27.3.1')
    await expect(canvas.getByTestId('pages-image')).toHaveTextContent(IMAGE)
    await expect(canvas.queryByTestId('pages-settings-reset')).toBeNull()
    await userEvent.click(canvas.getByTestId('pages-settings-save'))
    await expect(onsave).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Changed"
  args={{ port: '8080', changed: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('pages-settings-reset'))
    await expect(onreset).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('pages-settings-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<Story name="While a change is in flight" args={{ port: '8080', changed: true, working: true }} />

<Story
  name="A Docker elsewhere"
  args={{
    engine: { reachable: true, version: '27.3.1', host: 'tcp://nas.local:2375', problem: '' },
    hostDraft: 'tcp://nas.local:2375'
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('pages-engine')).toHaveTextContent('tcp://nas.local:2375')
  }}
/>
