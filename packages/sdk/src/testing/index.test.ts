import { describe, expect, test } from 'bun:test'
import { z } from 'zod'
import { defineManifest } from '../manifest'
import { defineMainApp } from '../app/main'
import { appApiFor, createMemoryHost, demoAgent } from './index'

const manifest = defineManifest({ id: 'echo', name: 'Echo', icon: 'plus', background: '#000' })
const main = defineMainApp((ctx) => {
  ctx.handle('echo', (payload) => ({ echoed: payload, count: (ctx.storage.get<number>('count') ?? 0) + 1 }))
  ctx.registerTool({
    name: 'shout',
    description: 'Upper-case some text and show it in the Echo window.',
    input: { text: z.string() },
    run: ({ text }) => {
      ctx.openWindow()
      ctx.emit('shown', text.toUpperCase())
      return { shouted: text.toUpperCase() }
    }
  })
})

describe('createMemoryHost', () => {
  test('routes renderer invokes to the app main module', async () => {
    const host = createMemoryHost({ apps: [{ manifest, main, storage: { count: 41 } }] })
    const api = appApiFor(host, 'echo')
    expect(await api.invoke('echo', 'hi')).toEqual({ echoed: 'hi', count: 42 })
    await expect(api.invoke('missing')).rejects.toThrow('no handler')
  })

  test('validates tool input with zod and namespaces tool ids', async () => {
    const host = createMemoryHost({ apps: [{ manifest, main }] })
    expect((await host.tools.list()).map((t) => t.id)).toEqual(['echo__shout'])
    await expect(host.tools.call('echo__shout', { text: 1 })).rejects.toThrow()
    expect(await host.tools.call('echo__shout', { text: 'hey' })).toEqual({ shouted: 'HEY' })
  })

  test('queues events emitted before the window subscribes, and asks to open the window', async () => {
    const host = createMemoryHost({ apps: [{ manifest, main }] })
    const opened: string[] = []
    host.apps.onOpenRequest((id) => opened.push(id))
    const api = appApiFor(host, 'echo')
    await host.tools.call('echo__shout', { text: 'late' })
    const seen = await new Promise((resolve) => api.on('shown', resolve))
    expect(seen).toBe('LATE')
    expect(opened).toEqual(['echo'])
  })

  test('plays a scripted agent turn: chunks, tool calls and permission requests', async () => {
    const host = createMemoryHost({ apps: [{ manifest, main }], agents: [demoAgent()] })
    const kinds: string[] = []
    host.agents.onUpdate(({ notification }) => kinds.push((notification.update as { sessionUpdate: string }).sessionUpdate))
    host.agents.onPermission(({ id }) => host.agents.respondPermission(id, 'allow'))
    const { sessionId } = await host.agents.newSession('claude')
    expect(await host.agents.prompt('claude', sessionId, 'delete my old files')).toBe('end_turn')
    expect(kinds).toContain('agent_message_chunk')
    expect(host.calls.some((c) => c.method === 'agents.respondPermission')).toBe(true)
  })
})
