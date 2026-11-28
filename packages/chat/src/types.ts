/** What an item is: what the person said, what the agent said, thought or did, or something the chat says. */
export type Kind = 'user' | 'assistant' | 'reasoning' | 'tool' | 'plan' | 'permission' | 'note' | 'error'

/**
 * One thing said or done in a chat, in the order it happened, as an app's backend sends it (Agents' and Copilot's
 * `Item`). `data` is JSON text, or empty: what the agent sent besides the text (lib/items.ts reads it).
 */
export interface Item {
  id: number
  chat: string
  kind: Kind
  text: string
  name: string
  data: string
  done: boolean
}

/** A chat as the thread shows it: its title, what was said and done in it, and whether the agent is answering or waits for an answer. */
export interface Conversation {
  chat: { title: string }
  items: Item[]
  /** The agent is answering. */
  busy: boolean
  /** The agent asked for a permission that has no answer yet. */
  waiting: boolean
}
