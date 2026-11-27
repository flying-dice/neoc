<script lang="ts">
  import { Containers, type ContainersEvents, type ContainersRpc } from './lib/containers.svelte'
  import ContainersPage from './pages/ContainersPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: ContainersRpc = {
    overview: () => RPC.send_request('containers', '_overview'),
    connect: (params) => RPC.send_request('containers', '_connect', params),
    start: (params) => RPC.send_request('containers', '_start', params),
    stop: (params) => RPC.send_request('containers', '_stop', params),
    restart: (params) => RPC.send_request('containers', '_restart', params),
    remove: (params) => RPC.send_request('containers', '_remove', params),
    create: (params) => RPC.send_request('containers', '_create', params),
    recreate: (params) => RPC.send_request('containers', '_recreate', params),
    images: () => RPC.send_request('containers', '_images'),
    pullImage: (params) => RPC.send_request('containers', '_image_pull', params),
    removeImage: (params) => RPC.send_request('containers', '_image_remove', params),
    pruneImages: () => RPC.send_request('containers', '_images_prune'),
    volumes: () => RPC.send_request('containers', '_volumes'),
    createVolume: (params) => RPC.send_request('containers', '_volume_create', params),
    removeVolume: (params) => RPC.send_request('containers', '_volume_remove', params),
    pruneVolumes: () => RPC.send_request('containers', '_volumes_prune'),
    networks: () => RPC.send_request('containers', '_networks'),
    createNetwork: (params) => RPC.send_request('containers', '_network_create', params),
    removeNetwork: (params) => RPC.send_request('containers', '_network_remove', params),
    startStack: (params) => RPC.send_request('containers', '_stack_start', params),
    stopStack: (params) => RPC.send_request('containers', '_stack_stop', params),
    restartStack: (params) => RPC.send_request('containers', '_stack_restart', params),
    removeStack: (params) => RPC.send_request('containers', '_stack_remove', params),
    details: (params) => RPC.send_request('containers', '_details', params),
    logs: (params) => RPC.send_request('containers', '_logs', params)
  }
  const events: ContainersEvents = {
    on(name: string, fn: (progress: VosApp.Containers.PullingParams) => void): () => void {
      if (name === 'pulling') return RPC.on_notification('containers', '_pulling', fn)
      const heard = (): void => (fn as () => void)()
      return name === 'changed' ? RPC.on_notification('containers', 'changed', heard) : vos.host.on(name, heard)
    }
  }
  const containers = new Containers(rpc, events)
</script>

<ContainersPage {containers} />
