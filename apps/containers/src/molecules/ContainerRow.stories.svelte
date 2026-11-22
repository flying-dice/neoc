<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  import type { Container } from 'app:rpc'
  import ContainerRow from './ContainerRow.svelte'
  import { container } from '../lib/fakeApi'

  interface Args {
    container: Container
    busy?: boolean
    onopen: () => void
    ontoggle: () => void
  }

  const onopen = fn<() => void>()
  const ontoggle = fn<() => void>()

  /** One container in a list. It keeps nothing: the container comes in, and what the person did to it goes out. */
  const { Story } = defineMeta({
    title: 'Molecules/ContainerRow',
    component: ContainerRow,
    tags: ['autodocs'],
    args: { container: container('web', 'nginx:1.27', 'running', { ports: ['8080:80'] }), onopen, ontoggle },
    beforeEach: () => {
      onopen.mockClear()
      ontoggle.mockClear()
    },
    render: template
  })
</script>

{#snippet template(args: Args)}
  <ul style="width:460px;margin:0;padding:0 12px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <ContainerRow {...args} />
  </ul>
{/snippet}

<!-- A running container: it can be stopped. -->
<Story
  name="Running"
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('img', { name: 'Running' })).toBeVisible()
    await expect(canvas.getByTestId('containers-item-web-ports')).toHaveTextContent('8080:80')
    await expect(canvas.getByTestId('containers-item-web-stop')).toHaveProperty('label', 'Stop web')
    await expect(canvas.queryByTestId('containers-item-web-start')).toBeNull()
  }}
/>

<!-- A stopped one: it can be started, and publishes nothing. -->
<Story
  name="Stopped"
  args={{ container: container('web', 'nginx:1.27', 'exited') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByRole('img', { name: 'Stopped' })).toBeVisible()
    await expect(canvas.getByTestId('containers-item-web-start')).toHaveProperty('label', 'Start web')
    await expect(canvas.queryByTestId('containers-item-web-ports')).toBeNull()
  }}
/>

<Story name="Restarting" args={{ container: container('web', 'nginx:1.27', 'restarting') }} />

<!-- A call is in flight: the button gives way to a spinner, in the same room. -->
<Story
  name="Busy"
  args={{ busy: true }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-item-web-busy')).toBeVisible()
    await expect(canvas.queryByTestId('containers-item-web-stop')).toBeNull()
  }}
/>

<!-- Names, images and ports as long as they come: each is cut short, and the button stays. -->
<Story
  name="Long"
  args={{
    container: container('a-compose-project-with-a-long-name-and-a-service-1', 'registry.example.org:5000/some/team/image-with-a-name:2024.07.0-alpine', 'running', {
      ports: ['8080:80', '8443:443', '9100:9100/udp']
    })
  }}
/>

<!-- An image with no tag left is shown as the start of its digest. -->
<Story
  name="Untagged image"
  args={{ container: container('old', `sha256:${'0123456789ab'.repeat(5)}0123`, 'exited') }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-item-old-open')).toHaveTextContent('0123456789ab ·')
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-item-web-open'))
    await expect(onopen).toHaveBeenCalledTimes(1)
    await expect(ontoggle).not.toHaveBeenCalled()
    await userEvent.click(canvas.getByTestId('containers-item-web-stop'))
    await expect(ontoggle).toHaveBeenCalledTimes(1)
    await expect(onopen).toHaveBeenCalledTimes(1)
  }}
/>
