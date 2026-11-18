/**
 * The transcript as the kit draws it: one turn is the person's message, a single activity line (counts, never a
 * list of tool calls) and the agent's reply merged into one bubble. Permission prompts and errors stay where they
 * happened.
 */
import type { ChatBlock } from '@agentos/ui'
import type { ChatItem, ChatStatus } from './session'

type Activity = Extract<ChatBlock, { kind: 'activity' }>
type Reply = Extract<ChatBlock, { kind: 'reply' }>

/** Agent-internal plumbing that is not a user-meaningful tool call. */
const INTERNAL_TOOLS = /^(ToolSearch|TodoWrite)$/

export function toBlocks(items: readonly ChatItem[]): ChatBlock[] {
  const blocks: ChatBlock[] = []
  let activity: Activity | null = null
  let reply: Reply | null = null
  const ensureActivity = (id: string): Activity => {
    if (!activity) {
      activity = {
        kind: 'activity',
        id: `activity-${id}`,
        reasoning: 0,
        tools: 0,
        failed: 0,
        running: false
      }
      blocks.push(activity)
    }
    return activity
  }
  for (const item of items) {
    switch (item.kind) {
      case 'user':
        blocks.push({ kind: 'user', id: item.id, text: item.text })
        activity = null
        reply = null
        ensureActivity(item.id)
        break
      case 'thought':
        ensureActivity(item.id).reasoning++
        break
      case 'tool': {
        const a = ensureActivity(item.id)
        if (item.status === 'pending' || item.status === 'in_progress') a.running = true
        if (INTERNAL_TOOLS.test(item.name)) break
        a.tools++
        if (item.status === 'failed') a.failed++
        break
      }
      case 'agent':
        if (!item.text) break
        if (reply) reply.text += `\n\n${item.text}`
        else blocks.push((reply = { kind: 'reply', id: `reply-${item.id}`, text: item.text }))
        break
      case 'permission':
        blocks.push({
          kind: 'permission',
          id: item.id,
          title: item.title,
          options: item.options,
          answer: item.answer
        })
        break
      case 'error':
        blocks.push({ kind: 'error', id: item.id, text: item.text })
        break
    }
  }
  return blocks
}

/** The activity line that is live: the newest one, while a run is going (or starting). */
export function liveActivityId(
  blocks: readonly ChatBlock[],
  status: ChatStatus
): string | undefined {
  if (status !== 'running' && status !== 'connecting') return undefined
  return blocks.findLast((b) => b.kind === 'activity')?.id
}
