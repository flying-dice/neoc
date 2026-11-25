/**
 * A stand-in for the Example app's backend, for stories and unit tests: the same answers as server/main.luau, kept in
 * the page, with no host. It is what lets every state of the page be drawn and tested without a running vos.
 */
import type { Part } from 'app:rpc'
import type { ExampleEvents, ExampleRpc } from './example.svelte'

export interface FakeBackend {
  readonly rpc: ExampleRpc
  readonly events: ExampleEvents
  /** Send an event to the window, as the host does (`host:reconnected`). */
  emit(name: string): void
  /** The calls the backend answered, in order, by method name. */
  readonly calls: string[]
}

export interface FakeOptions {
  parts?: readonly Part[]
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
}

/** A failure the backend answers a call with (`ctx:fail`), as the page's client rejects with it. */
class Refused extends Error {
  readonly code = -32010
}

/** The parts server/parts.luau serves, shortened. */
export const PARTS: readonly Part[] = [
  {
    id: 'contract',
    title: 'Contract',
    icon: 'doc',
    summary: 'openrpc.json names the app and every method the page may call.',
    facts: [
      { label: 'File', value: 'openrpc.json' },
      { label: 'Methods', value: 'parts, greet' }
    ]
  },
  {
    id: 'backend',
    title: 'Backend',
    icon: 'terminal',
    summary: 'Luau, run by the host: one handler per method.',
    facts: [{ label: 'Entry', value: 'server/main.luau' }]
  },
  {
    id: 'page',
    title: 'Page',
    icon: 'square.grid',
    summary: 'A Vite page drawn with the kit’s ui-* elements.',
    facts: [{ label: 'Entry', value: 'src/main.ts' }]
  }
]

export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const parts = options.parts ?? PARTS
  const calls: string[] = []
  const listeners = new Map<string, Set<() => void>>()
  const later = <T>(answer: () => T): Promise<T> =>
    new Promise((resolve, reject) =>
      setTimeout(() => {
        try {
          resolve(answer())
        } catch (e) {
          reject(e)
        }
      }, options.pace ?? 0)
    )

  const rpc: ExampleRpc = {
    parts: () => {
      calls.push('parts')
      return later(() => parts.map((p) => ({ ...p, facts: [...p.facts] })))
    },
    greet: ({ name }) => {
      calls.push(`greet ${name}`)
      return later(() => {
        const trimmed = name.trim()
        if (trimmed === '') throw new Refused('Type a name to greet.')
        return { message: `Hello, ${trimmed}! This came from the Example app's Luau backend.` }
      })
    }
  }
  const events: ExampleEvents = {
    on: (name, handler) => {
      const set = listeners.get(name) ?? new Set<() => void>()
      listeners.set(name, set)
      const listener = (): void => void handler(undefined as never)
      set.add(listener)
      return () => void set.delete(listener)
    }
  }
  return { rpc, events, calls, emit: (name) => listeners.get(name)?.forEach((l) => l()) }
}

/** A backend that cannot be reached at all: every call fails the way the host's does. */
export function createFailingBackend(message: string): FakeBackend {
  const fail = (): Promise<never> => Promise.reject(new Error(message))
  return { rpc: { parts: fail, greet: fail }, events: { on: () => () => {} }, calls: [], emit: () => {} }
}
