<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import { expect, fn } from 'storybook/test'
  type Network = VosApp.Containers.Network
  import NetworkList from './NetworkList.svelte'
  import { OWN_NETWORKS, network } from '../lib/fakeApi'

  interface Args {
    networks: readonly Network[]
    back?: boolean
    onback?: () => void
    onremove: (network: Network) => void
    onnew: () => void
  }

  const onback = fn<() => void>()
  const onremove = fn<(network: Network) => void>()
  const onnew = fn<() => void>()
  const heard = [onback, onremove, onnew]

  const NETWORKS: Network[] = [
    { ...network('backend', { subnet: '172.30.0.0/16', gateway: '172.30.0.1', internal: true }), containers: [] },
    { ...network('blog_default', { subnet: '172.18.0.0/16', gateway: '172.18.0.1' }), containers: ['blog-db-1', 'blog-web-1'] },
    ...OWN_NETWORKS.map((n) => ({ ...n, containers: n.name === 'bridge' ? ['pihole', 'scratch'] : [] })),
    { ...network('lan', { driver: 'macvlan', subnet: '192.168.1.0/24', gateway: '192.168.1.1' }), containers: [] }
  ]

  /** The networks Docker has. It keeps nothing. */
  const { Story } = defineMeta({
    title: 'Organisms/NetworkList',
    component: NetworkList,
    tags: ['autodocs'],
    args: { networks: NETWORKS, onback, onremove, onnew },
    beforeEach: () => heard.forEach((spy) => spy.mockClear()),
    render: template
  })
</script>

{#snippet template(args: Args)}
  <div style="width:600px;height:560px;background:var(--bg);color:var(--label);font:var(--type-body)">
    <NetworkList {...args} />
  </div>
{/snippet}

<!-- Every network: its driver and scope, its subnet and gateway, the containers on it. Docker's own cannot be removed. -->
<Story
  name="Networks"
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-summary')).toHaveTextContent('6 networks')
    await expect(canvas.getByTestId('containers-network-blog_default')).toHaveTextContent('172.18.0.0/16 · gateway 172.18.0.1')
    await expect(canvas.getByTestId('containers-network-blog_default-note')).toHaveTextContent('Containers: blog-db-1 and blog-web-1')
    await expect(canvas.getByTestId('containers-network-backend')).toHaveTextContent('bridge · local · internal')
    await expect(canvas.getByTestId('containers-network-host')).toHaveTextContent('no subnet')
    for (const own of ['bridge', 'host', 'none']) await expect(canvas.queryByTestId(`containers-network-${own}-remove`)).toBeNull()
    await expect(canvas.getByTestId('containers-network-lan-remove')).toBeVisible()
  }}
/>

<Story
  name="No networks"
  args={{ networks: [] }}
  play={async ({ canvas }) => {
    await expect(canvas.getByTestId('containers-networks-empty')).toBeVisible()
  }}
/>

<Story
  name="Tapping"
  tags={['play-only']}
  play={async ({ canvas, userEvent }) => {
    await userEvent.click(canvas.getByTestId('containers-networks-new'))
    await expect(onnew).toHaveBeenCalledTimes(1)
    await userEvent.click(canvas.getByTestId('containers-network-backend-remove'))
    await expect(onremove).toHaveBeenCalledWith(NETWORKS[0])
  }}
/>
