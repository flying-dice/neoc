<script lang="ts">
  import { Agents, type AgentsEvents, type AgentsRpc } from './lib/agents.svelte'
  import AgentsPage from './pages/AgentsPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: AgentsRpc = {
    agents: () => RPC.send_request('agents', '_agents'),
    addAgent: (params) => RPC.send_request('agents', '_add_agent', params),
    updateAgent: (params) => RPC.send_request('agents', '_update_agent', params),
    removeAgent: (params) => RPC.send_request('agents', '_remove_agent', params),
    chats: () => RPC.send_request('agents', '_chats'),
    start: (params) => RPC.send_request('agents', '_start', params),
    open: (params) => RPC.send_request('agents', '_open', params),
    send: (params) => RPC.send_request('agents', '_send', params),
    answer: (params) => RPC.send_request('agents', '_answer', params),
    stop: (params) => RPC.send_request('agents', '_stop', params),
    removeChat: (params) => RPC.send_request('agents', '_remove_chat', params)
  }
  // the app's notifications come from the bus, the host's events (`host:...`) from the host
  const events: AgentsEvents = {
    on: (name, fn) => {
      switch (name) {
        case 'item':
          return RPC.on_notification('agents', '_item', fn)
        case 'piece':
          return RPC.on_notification('agents', '_piece', fn)
        case 'turn':
          return RPC.on_notification('agents', '_turn', fn)
        default:
          return vos.host.on(name, fn)
      }
    }
  }
  const agents = new Agents(rpc, events)
</script>

<AgentsPage {agents} />
