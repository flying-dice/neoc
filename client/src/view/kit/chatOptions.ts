/**
 * What `registerChat` was given, per registry: the adapter reads it from the registry in its view context, so two
 * registries (the app's and a story's, with a fake host) never share a transport.
 */
import type { ComponentRegistry } from '../registry'
import type { FetchLike } from './agui/session'

export interface ChatOptions {
  /** The transport to `/agui`; defaults to the global `fetch`. */
  fetch?: FetchLike
  /** Prefix of every `/agui` route (`''` is same origin). */
  baseUrl?: string
}

const options = new WeakMap<ComponentRegistry, ChatOptions>()

export const setChatOptions = (registry: ComponentRegistry, o: ChatOptions): void =>
  void options.set(registry, o)

export const chatOptionsOf = (registry: ComponentRegistry): ChatOptions =>
  options.get(registry) ?? {}
