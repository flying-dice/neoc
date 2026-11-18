/**
 * `ChatTranscript` (tier 2): a chat bound to a `thread` source. `kit.ts` calls `registerChat(registry)`; a story passes
 * its own `fetch` to talk to a fake host.
 *
 *   props   thread: source id (kind "thread", params {thread, agent})   placeholder: text   composer: bool (default true)
 *   events  send {text}   stop
 */
import type { ComponentRegistry } from '../registry'
import ChatTranscript from '../templates/ChatTranscript.svelte'
import { setChatOptions, type ChatOptions } from './chatOptions'

export type { ChatOptions } from './chatOptions'

export function registerChat(
  registry: ComponentRegistry,
  options: ChatOptions = {}
): ComponentRegistry {
  setChatOptions(registry, options)
  return registry.register('ChatTranscript', { component: ChatTranscript, since: '1.0' })
}
