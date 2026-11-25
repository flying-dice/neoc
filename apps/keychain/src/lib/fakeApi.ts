/**
 * A stand-in for the Keychain app's backend, for stories and unit tests: a keychain and the app's list of it kept in
 * the page, with no host and no operating system. It follows the backend's rules (server/items.luau): a password the
 * keychain has already is not replaced, and an item without a password must be one the keychain has.
 */
import type { Item, Overview } from '@keychain/rpc'
import type { KeychainEvents, KeychainRpc } from './keychain.svelte'

export interface FakeBackend {
  readonly rpc: KeychainRpc
  readonly events: KeychainEvents
  /** Send an event to the window, as the host does (`host:reconnected`) or a tool of the backend (`changed`). */
  emit(name: string, data?: unknown): void
  /** Another program keeps a password in the keychain: the app does not show it until it is added. */
  outside(service: string, account: string, password: string): void
  /** Another program deletes a password the app shows: the app is not told. */
  lost(service: string, account: string): void
  /** An agent adds an item: the windows are told. */
  elsewhere(service: string, account: string, password: string): void
  /** The password the keychain has for a service and an account. */
  kept(service: string, account: string): string | undefined
  /** How many times a password was read from the keychain for the window. */
  reads(): number
}

export interface FakeOptions {
  /** The items the app shows, each with its password in the keychain (default: `ITEMS`). */
  items?: readonly Kept[]
  /** This machine has no keychain, for this reason. */
  problem?: string
  /** Milliseconds a call takes (default: none, it answers on the next turn of the event loop). */
  pace?: number
}

/** An item and the password the keychain has for it. */
export interface Kept extends Item {
  password: string
}

/** A failure the backend answers a call with (`ctx:fail`), as the page's client rejects with it. */
class Refused extends Error {
  readonly code = -32010
}

export const NO_KEYCHAIN =
  'This machine has no keychain the app can use. On Linux it needs a Secret Service, like GNOME Keyring or KWallet, to be running.'

/** An item, for a story's or a test's starting point. */
export const kept = (id: number, service: string, account: string, password: string): Kept => ({ id, service, account, password })

/** What a home lab keeps: a registry, a router, a backup. */
export const ITEMS: readonly Kept[] = [
  kept(1, 'backup.example.com', 'ada', 'correct horse battery staple'),
  kept(2, 'ghcr.io', 'ada-lovelace', 'ghp_x7Kq2mVn9RtL4wZs'),
  kept(3, 'router.home', 'admin', 'tr0ub4dor&3')
]

const key = (service: string, account: string): string => `${service}\n${account}`

/** A backend in the page: every call is answered on a later turn of the event loop, as a real answer would be. */
export function createFakeBackend(options: FakeOptions = {}): FakeBackend {
  const starting = options.items ?? ITEMS
  let items: Item[] = starting.map(({ id, service, account }) => ({ id, service, account }))
  const keychain = new Map(starting.map((item) => [key(item.service, item.account), item.password]))
  const problem = options.problem ?? ''
  const pace = options.pace ?? 0
  let next = Math.max(0, ...items.map((item) => item.id)) + 1
  let reads = 0
  const listeners = new Map<string, Set<(data: unknown) => void>>()

  const emit = (name: string, data?: unknown): void => listeners.get(name)?.forEach((fn) => fn(data))
  const answered = <T>(answer: () => T): Promise<T> =>
    new Promise((resolve, reject) =>
      setTimeout(() => {
        try {
          resolve(answer())
        } catch (e) {
          reject(e as Error)
        }
      }, pace)
    )

  const overview = (): Overview => ({
    keychain: { available: problem === '', problem },
    items: [...items]
      .sort((a, b) => a.service.localeCompare(b.service, 'en', { sensitivity: 'base' }) || a.account.localeCompare(b.account, 'en', { sensitivity: 'base' }))
      .map((item) => ({ ...item }))
  })
  const found = (id: number): Item => {
    const item = items.find((i) => i.id === id)
    if (!item) throw new Refused('There is no such item.')
    return item
  }
  const add = (service: string, account: string, password: string): void => {
    const what = service.trim()
    const whose = account.trim()
    if (what === '') throw new Refused('Say what the password is for.')
    if (whose === '') throw new Refused('Say whose password it is.')
    if (items.some((i) => i.service === what && i.account === whose)) throw new Refused('This item is here already.')
    if (problem !== '') throw new Refused(problem)
    const has = keychain.has(key(what, whose))
    if (password === '') {
      if (!has) throw new Refused('The keychain has no password for this service and account. Type one to keep it.')
    } else {
      if (has) {
        throw new Refused('The keychain has a password for this service and account already. Leave the password empty to show it here.')
      }
      keychain.set(key(what, whose), password)
    }
    items = [...items, { id: next, service: what, account: whose }]
    next += 1
  }

  const rpc: KeychainRpc = {
    overview: () => answered(overview),
    add: ({ service, account, password }) =>
      answered(() => {
        add(service, account, password ?? '')
        return overview()
      }),
    reveal: ({ item: id }) =>
      answered(() => {
        const item = found(id)
        if (problem !== '') throw new Refused(problem)
        reads += 1
        const password = keychain.get(key(item.service, item.account))
        if (password === undefined) throw new Refused('The keychain no longer has this item.')
        return { password }
      }),
    remove: ({ item: id }) =>
      answered(() => {
        const item = found(id)
        if (problem !== '') throw new Refused(problem)
        keychain.delete(key(item.service, item.account))
        items = items.filter((i) => i.id !== id)
        return overview()
      })
  }

  const events: KeychainEvents = {
    on(name, fn) {
      const heard = listeners.get(name) ?? new Set()
      listeners.set(name, heard)
      heard.add(fn)
      return () => heard.delete(fn)
    }
  }

  return {
    rpc,
    events,
    emit,
    outside: (service, account, password) => void keychain.set(key(service, account), password),
    lost: (service, account) => void keychain.delete(key(service, account)),
    elsewhere(service, account, password) {
      add(service, account, password)
      emit('changed')
    },
    kept: (service, account) => keychain.get(key(service, account)),
    reads: () => reads
  }
}

/** A backend that cannot be reached: every call is refused with `message`, as the host or a lost connection would. */
export function createFailingBackend(message: string): FakeBackend {
  const refuse = (): Promise<never> => Promise.reject(new Error(message))
  return {
    rpc: { overview: refuse, add: refuse, reveal: refuse, remove: refuse },
    events: { on: () => () => {} },
    emit: () => {},
    outside: () => {},
    lost: () => {},
    elsewhere: () => {},
    kept: () => undefined,
    reads: () => 0
  }
}
