<script lang="ts">
  import { Copilots, type CopilotEvents, type CopilotRpc } from './lib/copilot.svelte'
  import CopilotPage from './pages/CopilotPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: CopilotRpc = {
    engine: () => RPC.send_request('copilot', '_engine'),
    connect: (params) => RPC.send_request('copilot', '_connect', params),
    copilots: () => RPC.send_request('copilot', '_copilots'),
    addCopilot: (params) => RPC.send_request('copilot', '_add_copilot', params),
    updateCopilot: (params) => RPC.send_request('copilot', '_update_copilot', params),
    removeCopilot: (params) => RPC.send_request('copilot', '_remove_copilot', params),
    build: (params) => RPC.send_request('copilot', '_build', params),
    skills: () => RPC.send_request('copilot', '_skills'),
    addSkill: (params) => RPC.send_request('copilot', '_add_skill', params),
    updateSkill: (params) => RPC.send_request('copilot', '_update_skill', params),
    removeSkill: (params) => RPC.send_request('copilot', '_remove_skill', params),
    secrets: () => RPC.send_request('copilot', '_secrets'),
    chats: () => RPC.send_request('copilot', '_chats'),
    open: (params) => RPC.send_request('copilot', '_open', params),
    send: (params) => RPC.send_request('copilot', '_send', params),
    answer: (params) => RPC.send_request('copilot', '_answer', params),
    stop: (params) => RPC.send_request('copilot', '_stop', params),
    removeChat: (params) => RPC.send_request('copilot', '_remove_chat', params)
  }
  // the app's notifications come from the bus, the host's events (`host:...`) from the host
  const events: CopilotEvents = {
    on: (name, fn) => {
      switch (name) {
        case 'item':
          return RPC.on_notification('copilot', '_item', fn)
        case 'piece':
          return RPC.on_notification('copilot', '_piece', fn)
        case 'turn':
          return RPC.on_notification('copilot', '_turn', fn)
        case 'building':
          return RPC.on_notification('copilot', '_building', fn)
        default:
          return vos.host.on(name, fn)
      }
    }
  }
  const copilots = new Copilots(rpc, events)
</script>

<CopilotPage {copilots} onlink={(url) => void vos.host.open_link(url).catch(() => {})} />
