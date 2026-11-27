<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Network = VosApp.Containers.Network
  type Volume = VosApp.Containers.Volume
  import ContainerForm from './ContainerForm.svelte'
  import type { ContainerForm as Form } from '../lib/containers.svelte'
  import { OWN_NETWORKS, container, network, volume } from '../lib/fakeApi'
  import { blankDraft, blankEntry, blankMount, blankPort, type Draft } from '../lib/spec'

  interface Args {
    form: Form
    networks: readonly Network[]
    volumes: readonly Volume[]
    savable: boolean
    onsave: () => void
    oncancel: () => void
  }

  const onsave = fn<() => void>()
  const oncancel = fn<() => void>()

  const NETWORKS: Network[] = [...OWN_NETWORKS, network('blog_default')].map((n) => ({ ...n, containers: [] }))
  const VOLUMES: Volume[] = [volume('blog_db-data'), volume('site-data')].map((v) => ({ ...v, containers: [] }))
  const SITE = container('site', 'nginx:1.27', 'running', { ports: ['8088:80'] })

  /** A form with everything a container can be made with filled in. */
  function filled(): Draft {
    return {
      ...blankDraft(),
      image: 'nginx:1.27',
      name: 'site',
      command: "nginx -g 'daemon off;'",
      environment: [
        { ...blankEntry(), name: 'NGINX_HOST', value: 'example.org' },
        { ...blankEntry(), name: 'TZ', value: 'Europe/Amsterdam' }
      ],
      ports: [
        { ...blankPort(), host_port: '8088', container_port: '80' },
        { ...blankPort(), host_ip: '127.0.0.1', host_port: '8443', container_port: '443' }
      ],
      mounts: [
        { ...blankMount('bind'), source: '/srv/www', target: '/usr/share/nginx/html', read_only: true },
        { ...blankMount('volume'), source: 'site-data', target: '/data', fresh: false },
        { ...blankMount('volume'), source: 'site-cache', target: '/var/cache/nginx' }
      ],
      restart: 'on-failure',
      retries: '5',
      labels: [{ ...blankEntry(), name: 'com.example.owner', value: 'web' }],
      working_dir: '/usr/share/nginx',
      user: '101:101'
    }
  }
  const form = (draft: Draft, more: Partial<Form> = {}): Form => ({ mode: 'new', of: null, draft, failure: null, saving: false, pulling: null, ...more })
  const REFUSED = "Port 2: the machine's port must be a number from 1 to 65535, or empty for one Docker chooses."
  const input = (element: HTMLElement): HTMLInputElement => element.shadowRoot!.querySelector('input')!

  /** The form a container is made, changed or copied in. It keeps nothing: what is typed goes into the form it was given. */
  const { Story } = defineMeta({
    title: 'Organisms/ContainerForm',
    component: ContainerForm,
    tags: ['autodocs'],
    args: { form: form(blankDraft()), networks: NETWORKS, volumes: VOLUMES, savable: false, onsave, oncancel },
    beforeEach: () => {
      onsave.mockClear()
      oncancel.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:600px;height:640px;color:var(--label);font:var(--type-body)">
    <ContainerForm {...args} />
  </div>
{/snippet}

<!-- A new container: nothing to create until it names an image; it is started once it is made, on Docker's own network. -->
<Story
  name="Empty"
  play={async ({ canvas, userEvent }) => {
    await expect(canvas.getByTestId('containers-form-title')).toHaveTextContent('New Container')
    await expect(canvas.getByTestId('containers-form-save')).toHaveProperty('disabled', true)
    await expect(canvas.getByTestId('containers-form-start')).toHaveProperty('checked', true)
    await expect(canvas.getByTestId('containers-form-network')).toHaveProperty('value', 'bridge')
    await expect(canvas.queryByTestId('containers-form-recreate')).toBeNull()
    await expect(canvas.queryByTestId('containers-form-retries')).toBeNull()
    // rows are added, and taken out again
    await userEvent.click(canvas.getByTestId('containers-form-add-port'))
    await expect(canvas.getByTestId('containers-form-port-0')).toBeVisible()
    await userEvent.click(canvas.getByTestId('containers-form-port-0-remove'))
    await expect(canvas.queryByTestId('containers-form-port-0')).toBeNull()
    await userEvent.click(canvas.getByTestId('containers-form-cancel'))
    await expect(oncancel).toHaveBeenCalledTimes(1)
  }}
/>

<!-- Everything a container can be made with: variables, ports, a folder and volumes, a restart policy with its retries, labels and the advanced settings. -->
<Story
  name="Filled"
  args={{ form: form(filled()), savable: true }}
  play={async ({ canvas, userEvent }) => {
    await expect(input(canvas.getByTestId('containers-form-image'))).toHaveValue('nginx:1.27')
    await expect(input(canvas.getByTestId('containers-form-env-0-name'))).toHaveValue('NGINX_HOST')
    await expect(input(canvas.getByTestId('containers-form-port-1-host-ip'))).toHaveValue('127.0.0.1')
    await expect(input(canvas.getByTestId('containers-form-mount-0-source'))).toHaveValue('/srv/www')
    await expect(canvas.getByTestId('containers-form-mount-0-read-only')).toHaveProperty('pressed', true)
    // a volume that is there is picked; a new one is named
    await expect(canvas.getByTestId('containers-form-mount-1-volume')).toHaveProperty('value', 'site-data')
    await expect(input(canvas.getByTestId('containers-form-mount-2-source'))).toHaveValue('site-cache')
    await expect(input(canvas.getByTestId('containers-form-retries'))).toHaveValue('5')
    await userEvent.click(canvas.getByTestId('containers-form-save'))
    await expect(onsave).toHaveBeenCalledTimes(1)
  }}
/>

<!-- The backend refused what was typed: its reason, at the top of the form, and the form stays as it was. -->
<Story
  name="With errors"
  args={{
    form: form({ ...filled(), ports: [{ ...blankPort(), host_port: '8088', container_port: '80' }, { ...blankPort(), host_port: '99999', container_port: '443' }] }, { failure: REFUSED }),
    savable: true
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-form-failure')).toHaveTextContent(REFUSED)
    await expect(input(canvas.getByTestId('containers-form-port-1-host-port'))).toHaveValue('99999')
  }}
/>

<!-- Saving: the image is pulled first, and how far it is is shown; nothing can be changed meanwhile. -->
<Story
  name="Pulling the image"
  args={{
    form: form(filled(), { saving: true, pulling: { image: 'nginx:1.27', status: 'Downloading', current: 18_400_000, total: 48_000_000 } }),
    savable: false
  }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-form-pulling')).toHaveTextContent('Pulling nginx:1.27')
    await expect(canvas.getByTestId('containers-form-pulling')).toHaveTextContent('18.4 MB of 48.0 MB')
    await expect(canvas.getByTestId('containers-form-saving')).toBeVisible()
    await expect(canvas.getByTestId('containers-form-cancel')).toHaveProperty('disabled', true)
  }}
/>

<!-- A container that is there, changed: the form says plainly that saving makes it again. -->
<Story
  name="Editing a container"
  args={{ form: form(filled(), { mode: 'edit', of: SITE }), savable: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-form-title')).toHaveTextContent('Edit site')
    await expect(canvas.getByTestId('containers-form-recreate')).toHaveTextContent('saving makes it again')
    await expect(canvas.getByTestId('containers-form-recreate')).toHaveTextContent('Anything it keeps outside a volume is lost')
    await expect(canvas.getByTestId('containers-form-save')).toHaveTextContent('Save')
  }}
/>
