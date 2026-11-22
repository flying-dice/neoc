/** The chat's view types (moved from packages/ui with the chat components). */

/** One choice of a select-like session setting. */
export interface ChatChoice {
  value: string
  name: string
  description?: string | null
  group?: string
}

/** An agent session setting (model, effort, ...): a select of choices or a boolean. */
export type ChatSetting =
  | {
      id: string
      name: string
      description?: string | null
      category?: string | null
      kind: 'select'
      current: string
      options: ChatChoice[]
    }
  | {
      id: string
      name: string
      description?: string | null
      category?: string | null
      kind: 'boolean'
      current: boolean
    }

export interface ChatMode {
  current: string
  available: { id: string; name: string; description?: string | null }[]
}

export interface ChatCommand {
  name: string
  description: string
  hint?: string | null
}

export interface ChatUsage {
  used: number
  size: number
  cost?: { amount: number; currency: string } | null
}

/** What a permission request offers: `kind` is `allow_once`, `allow_always`, `reject_once` or `reject_always`. */
export interface ChatPermissionOption {
  optionId: string
  name: string
  kind: string
}

/** A turn is the person's message, one activity line (counts only), and the agent's reply in one bubble. */
export type ChatBlock =
  | { kind: 'user'; id: string; text: string }
  | { kind: 'reply'; id: string; text: string }
  | {
      kind: 'activity'
      id: string
      reasoning: number
      tools: number
      failed: number
      running: boolean
    }
  | {
      kind: 'permission'
      id: string
      title: string
      options: ChatPermissionOption[]
      /** What was chosen, once answered. */
      answer?: string
    }
  | { kind: 'error'; id: string; text: string }
