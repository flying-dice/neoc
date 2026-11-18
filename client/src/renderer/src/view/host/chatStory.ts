/**
 * Storybook and test support for `ChatTranscript`: the chat window on the in-memory `/rpc` host (`ChatApp`) and a
 * scripted `/agui` host (`FakeAgui`) that the chat reaches through `fetch`. The real client talks to the Rust host.
 */
import type { Interrupt } from '@ag-ui/core'
import { memoryStorage } from '../core/clientState'
import { Session } from '../core/session'
import type { ViewCore } from '../core/view'
import { registerChat } from '../kit/chat'
import { scopedThread } from '../kit/agui/session'
import { parseThreadState, type ThreadState } from '../kit/agui/schema'
import { registerStarterKit } from '../kit'
import { createRegistry, type ComponentRegistry } from '../registry'
import { reactiveStore } from '../store.svelte'
import { ChatApp, type ChatAppOptions } from './chatApp'
import { FakeAgui, type FakeAguiOptions, type RunScript } from './fakeAgui'
import { FakeHost } from './fakeHost'
import { createMemoryLink } from './wire'

const STORY_APP = 'agents'

/** What the agent offers in a new chat: a mode, a model, an effort, commands and a little context used. */
export const demoState = (): ThreadState =>
  parseThreadState({
    session: {
      agent: { name: 'Claude', version: '0.108.0' },
      modes: {
        current: 'default',
        available: [
          { id: 'default', name: 'Default' },
          { id: 'plan', name: 'Plan' },
          { id: 'acceptEdits', name: 'Accept edits' }
        ]
      },
      config: [
        {
          id: 'model',
          name: 'Model',
          category: 'model',
          kind: 'select',
          current: 'large',
          options: [
            { value: 'large', name: 'Large' },
            { value: 'small', name: 'Small' }
          ]
        },
        {
          id: 'effort',
          name: 'Effort',
          category: 'thought_level',
          kind: 'select',
          current: 'medium',
          options: [
            { value: 'low', name: 'Low' },
            { value: 'medium', name: 'Medium' },
            { value: 'high', name: 'High' }
          ]
        }
      ],
      commands: [
        { name: 'compact', description: 'Summarise the conversation to free context' },
        { name: 'init', description: 'Set up this project' }
      ],
      usage: { used: 24000, size: 200000 }
    },
    plan: null
  })

export interface ChatScenario extends ChatAppOptions {
  agui?: FakeAguiOptions
  /** Messages and interrupts the thread already has when the chat opens. */
  history?: Record<string, unknown>[]
  interrupts?: Interrupt[]
  onRun?: RunScript
  onResume?: RunScript
  /** The host refuses to start the thread with this message (502) until the override is removed. */
  openError?: string
  latencyMs?: number
}

export interface ChatStoryHandle {
  view: ViewCore
  app: ChatApp
  agui: FakeAgui
  registry: ComponentRegistry
  /** Stop the session (the story unmounted). */
  dispose(): void
}

/** The story app is `agents`; the `/agui` id of its thread `id` (default `thread-1`), as the renderer scopes it. */
export const storyThread = (id = 'thread-1'): string => scopedThread(STORY_APP, id) ?? id
/** The `/agui` path of the story thread, encoded the way the session requests it. */
export const storyThreadPath = (id?: string): string => `/agui/threads/${encodeURIComponent(storyThread(id))}`

export async function openChatStory(scenario: ChatScenario = {}): Promise<ChatStoryHandle> {
  const agui = new FakeAgui({ state: demoState(), ...scenario.agui })
  // the renderer scopes an app's thread ids (`agents:thread-1`), so the fake host holds the scoped id
  const thread = agui.thread(storyThread(scenario.thread), scenario.agent ?? 'claude')
  thread.messages = scenario.history ?? []
  thread.interrupts = scenario.interrupts ?? []
  if (scenario.openError) {
    const error = scenario.openError
    agui.overrides.set('POST /agui/threads', () => Response.json({ error }, { status: 502 }))
  }
  if (scenario.onRun) agui.onRun = scenario.onRun
  if (scenario.onResume) agui.onResume = scenario.onResume

  const app = new ChatApp(scenario)
  const host = new FakeHost(app)
  const link = createMemoryLink({ latencyMs: scenario.latencyMs })
  link.onConnection((c) => host.accept(c))
  const session = new Session({
    transport: link.transport,
    locale: 'en-GB',
    timeZone: 'UTC',
    createStore: reactiveStore,
    storage: memoryStorage(),
    onError: () => undefined
  })
  session.start()
  const registry = registerChat(registerStarterKit(createRegistry()), { fetch: agui.fetch })
  const view = await session.openView({
    app: STORY_APP,
    window: 'main',
    windowId: 'w1',
    size: { w: 480, h: 720 }
  })
  return { view, app, agui, registry, dispose: () => session.stop() }
}
