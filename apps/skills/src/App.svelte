<script lang="ts">
  import { Skills, type SkillsEvents, type SkillsRpc } from './lib/skills.svelte'
  import SkillsPage from './pages/SkillsPage.svelte'

  // The composition root: the one place the state meets the real backend (the app's private methods on the bus, the
  // global `RPC`) and the host's events.
  const rpc: SkillsRpc = {
    skills: () => RPC.send_request('skills', '_skills'),
    addSkill: (params) => RPC.send_request('skills', '_add_skill', params),
    updateSkill: (params) => RPC.send_request('skills', '_update_skill', params),
    removeSkill: (params) => RPC.send_request('skills', '_remove_skill', params)
  }
  const events: SkillsEvents = { on: (name, fn) => vos.host.on(name, fn) }
  const skills = new Skills(rpc, events)
</script>

<SkillsPage {skills} />
