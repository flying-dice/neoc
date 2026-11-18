/**
 * Seed states for web mode (`?scenario=<name>`), shared by Playwright specs,
 * Storybook stories and the design research panel. Each scenario mutates the
 * renderer's stores after the desktop has mounted.
 */
import type { Conversation } from '../lib/agents.svelte'

export interface ScenarioStores {
  agentStore: { conversations: Conversation[]; selectedId: string | null }
}

const finishedChat = (): Conversation => ({
  id: 'convo-bill',
  agentId: 'claude',
  sessionId: undefined,
  title: 'Split a £186.40 bill between 7 people',
  status: 'idle',
  toolsConnected: true,
  items: [
    {
      kind: 'user',
      id: 'u1',
      text: 'Split a £186.40 bill between 7 people and show me the result.'
    },
    { kind: 'agent', id: 'a1', text: 'Let me work that out on the Calculator. ' },
    { kind: 'tool', id: 't1', title: 'Calculator · show on calculator', status: 'completed' },
    {
      kind: 'agent',
      id: 'a2',
      text: 'Each person pays **£26.63** (26.6286 rounded). It’s on the Calculator now.'
    }
  ]
})

const waitingChat = (): Conversation => ({
  id: 'convo-cleanup',
  agentId: 'claude',
  title: 'Delete old screenshots',
  status: 'waiting',
  toolsConnected: true,
  items: [
    { kind: 'user', id: 'u1', text: 'Delete the screenshots on my desktop older than a week.' },
    { kind: 'agent', id: 'a1', text: 'I found **14 screenshots** older than 7 days.' },
    {
      kind: 'permission',
      id: 'p1',
      requestId: '00000000-0000-4000-8000-000000000001',
      title: 'Move 14 files to Recently Deleted',
      options: [
        { optionId: 'allow', name: 'Allow', kind: 'allow_once' },
        { optionId: 'reject', name: 'Deny', kind: 'reject_once' }
      ]
    }
  ]
})

const errorChat = (): Conversation => ({
  id: 'convo-error',
  agentId: 'claude',
  title: 'Summarise my inbox',
  status: 'error',
  items: [
    { kind: 'user', id: 'u1', text: 'Summarise my inbox.' },
    { kind: 'error', id: 'e1', text: 'Agent exited (code 1)' }
  ]
})

export const scenarios: Record<string, (stores: ScenarioStores) => void> = {
  /** No conversations yet (the default). */
  empty: () => {},
  /** A finished chat that used the Calculator, selected. */
  'agents-chat': ({ agentStore }) => {
    agentStore.conversations = [finishedChat()]
    agentStore.selectedId = 'convo-bill'
  },
  /** Three chats: one waiting for permission (selected), one done, one failed. */
  'agents-busy': ({ agentStore }) => {
    agentStore.conversations = [waitingChat(), finishedChat(), errorChat()]
    agentStore.selectedId = 'convo-cleanup'
  }
}
