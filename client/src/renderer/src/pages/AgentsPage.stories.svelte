<script module lang="ts">
  import { defineMeta } from '@storybook/addon-svelte-csf'
  import AppWindow from '@agentos/storybook/AppWindow.svelte'
  import AgentsPage from './AgentsPage.svelte'
  import { apps } from '../lib/apps.svelte'
  import { agentStore } from '../lib/agents.svelte'
  import { scenarios } from '../web/scenarios'

  const app = apps.agents
  const seed = (name: string) => () => {
    agentStore.conversations = []
    agentStore.selectedId = null
    scenarios[name]({ agentStore })
  }

  /** Agents: chat with ACP agents (scripted in memory). */
  const { Story } = defineMeta({
    title: 'Apps/Agents',
    component: AgentsPage,
    tags: ['app'],
    args: { width: app.width, height: app.height },
    render: template
  })
</script>

{#snippet template(args: { width: number; height: number })}
  <AppWindow title={app.name} {...args} minWidth={app.minWidth} minHeight={app.minHeight}>
    <AgentsPage />
  </AppWindow>
{/snippet}

<Story name="New chat" beforeEach={seed('empty')} />

<Story name="Finished chat" beforeEach={seed('agents-chat')} />

<Story name="Waiting for permission" beforeEach={seed('agents-busy')} />
