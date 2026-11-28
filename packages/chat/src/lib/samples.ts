/**
 * Things an agent says and does in a chat, for stories and for an app's fake backend: a change to a file, a plan, a
 * permission asked for with its options, and a chat made of them.
 */
import type { Conversation, Item } from '../types'

/** The options an agent offers when it asks before it changes a file. */
export const OPTIONS = [
  { option_id: 'allow', name: 'Allow', kind: 'allow_once' },
  { option_id: 'always', name: 'Always allow', kind: 'allow_always' },
  { option_id: 'reject', name: 'Reject', kind: 'reject_once' }
]

/** A change to a file, as a tool call carries it. */
export const DIFF = {
  type: 'diff',
  path: '/home/ada/project/list.md',
  patch: '--- /home/ada/project/list.md\n+++ /home/ada/project/list.md\n@@ -1,2 +1,3 @@\n # Shopping\n-Bread\n+Bread\n+Milk\n',
  new_file: false
}

/** What an item is made from: its kind and whatever of the rest it has. */
export type Said = Pick<Item, 'kind'> & Partial<Omit<Item, 'id' | 'chat' | 'kind'>>

/** The item `said` makes, the `id`th of the chat. */
export const itemOf = (chat: string, id: number, said: Said): Item => ({ id, chat, text: '', name: '', data: '', done: true, ...said })

/** A tool call that adds milk to a list. */
export const TOOL: Said = {
  kind: 'tool',
  name: 'Add milk to list.md',
  data: JSON.stringify({ kind: 'edit', status: 'completed', content: [DIFF], locations: [{ path: DIFF.path, line: 2 }] })
}
/** A plan for the turn. */
export const PLAN: Said = {
  kind: 'plan',
  data: JSON.stringify({
    entries: [
      { content: 'Find the list', priority: 'high', status: 'completed' },
      { content: 'Add milk', priority: 'medium', status: 'in_progress' }
    ]
  })
}
/** A permission asked for, not answered yet. */
export const ASKING: Said = { kind: 'permission', text: 'Add milk to list.md', data: JSON.stringify({ kind: 'edit', options: OPTIONS }), done: false }

/** The items of a chat: each one said (a kind and its text) or made from what it has. */
export function itemsOf(chat: string, said: readonly (Said | readonly [kind: 'user' | 'assistant', text: string])[]): Item[] {
  return said.map((entry, i) => itemOf(chat, i + 1, Array.isArray(entry) ? { kind: entry[0], text: entry[1] } : (entry as Said)))
}

/** A chat with what was said and done in it, for a story's or a test's starting point. */
export function conversation(id: string, title: string, said: Parameters<typeof itemsOf>[1]): Conversation & { chat: { id: string } } {
  const items = itemsOf(id, said)
  return { chat: { id, title }, items, busy: false, waiting: items.some((item) => item.kind === 'permission' && !item.done) }
}
