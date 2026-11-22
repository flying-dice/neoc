<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Engine, Server } from 'app:rpc'
  import SettingsForm from './SettingsForm.svelte'
  import type { Problems, SettingsDraft } from '../lib/dns.svelte'
  import { IMAGE } from '../lib/fakeApi'

  interface Args {
    draft: SettingsDraft
    problems: Problems<'port' | 'upstreams'>
    refused: string | null
    changed: boolean
    working: boolean
    engine: Engine
    server: Server
    hostDraft: string
    ondraft: (change: Partial<SettingsDraft>) => void
    onsave: () => void
    onreset: () => void
    onhostdraft: (address: string) => void
    onconnect: () => void
  }

  const onsave = fn<() => void>()
  const onreset = fn<() => void>()

  /** How the server answers, and which Docker it runs in. */
  const { Story } = defineMeta({
    title: 'Organisms/SettingsForm',
    component: SettingsForm,
    tags: ['autodocs'],
    args: {
      draft: { port: '53', upstreams: '1.1.1.1, 1.0.0.1', logQueries: false },
      problems: { port: null, upstreams: null },
      refused: null,
      changed: false,
      working: false,
      engine: { reachable: true, version: '27.3.1', host: '', problem: '' },
      server: { enabled: true, state: 'running', status: 'Up 2 hours', current: true, image: IMAGE, problem: '' },
      hostDraft: '',
      ondraft: fn(),
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
  <div style="width:560px;height:520px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <SettingsForm {...args} />
  </div>
{/snippet}

<!-- As they are kept: there is nothing to save. -->
<Story
  name="As kept"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('dns-engine')).toHaveTextContent('Docker 27.3.1')
    await expect(canvas.queryByTestId('dns-settings-reset')).toBeNull()
    await expect(canvas.getByTestId('dns-settings-save')).toHaveProperty('disabled', true)
    await userEvent.click(canvas.getByTestId('dns-settings-save'))
    await expect(onsave).not.toHaveBeenCalled()
  }}
/>

<Story
  name="Changed"
  args={{ draft: { port: '5353', upstreams: '9.9.9.9', logQueries: true }, changed: true }}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('dns-settings-reset'))
    await expect(onreset).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('dns-settings-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<!-- A port that is none is said in the footer as it is typed, and Save cannot be pressed. -->
<Story
  name="A port that is none"
  args={{ draft: { port: '70000', upstreams: '1.1.1.1', logQueries: false }, problems: { port: 'A number from 1 to 65535.', upstreams: null }, changed: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('dns-settings-form')).toHaveProperty('footer', 'A number from 1 to 65535.')
    await expect(canvas.getByTestId('dns-settings-form')).toHaveProperty('footerTone', 'error')
    await expect(canvas.getByTestId('dns-settings-save')).toHaveProperty('disabled', true)
  }}
/>

<!-- A server that is no address is said once the person leaves the field; Save cannot be pressed either way. -->
<Story
  name="A server that is none"
  args={{
    draft: { port: '53', upstreams: 'dns.quad9.net', logQueries: false },
    problems: { port: null, upstreams: 'dns.quad9.net is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.' },
    changed: true
  }}
  play={async ({ canvas, userEvent }) => {
    const form = canvas.getByTestId('dns-settings-form')
    await expect(form).toHaveProperty('footerTone', 'note')
    await expect(canvas.getByTestId('dns-settings-save')).toHaveProperty('disabled', true)
    await userEvent.click(canvas.getByTestId('dns-upstreams').shadowRoot!.querySelector('input')!)
    await userEvent.tab()
    await expect(form).toHaveProperty('footer', 'dns.quad9.net is not an IP address, like 1.1.1.1 or 2606:4700:4700::1111.')
    await expect(form).toHaveProperty('footerTone', 'error')
    await userEvent.click(canvas.getByTestId('dns-settings-save'))
    await expect(onsave).not.toHaveBeenCalled()
  }}
/>

<Story
  name="In a Docker elsewhere"
  args={{ engine: { reachable: true, version: '27.3.1', host: 'tcp://nas.local:2375', problem: '' }, hostDraft: 'tcp://nas.local:2375' }}
/>
