/**
 * The `/agui` wire (docs/architecture/host-stack.md, "Agents over AG-UI"): what the host sends that AG-UI does not
 * define (session state, thread snapshot, permission options, our `vos.tool` custom event), validated with zod at
 * the edge. The standard events, interrupts and resume entries come from `@ag-ui/core` (types, plus its zod
 * `EventSchemas` for every frame); nothing here re-declares them.
 */
import type { Interrupt, ResumeEntry, RunAgentInput } from '@ag-ui/core'
import { EventSchemas } from '@ag-ui/core/schemas'
import { uniqueBy } from '@vos/ui'
import { z } from 'zod'

export { EventType } from '@ag-ui/core'
export { uniqueBy }
export type { Event as AguiEvent, Interrupt, ResumeEntry, RunAgentInput } from '@ag-ui/core'

// ── state the host keeps per thread (vos_domain::thread) ───────────────────────────────────────────────

const text = z.string()
const note = z.string().nullish()

export const choiceSchema = z.object({ value: text, name: text, description: note })

export const settingSchema = z.discriminatedUnion('kind', [
  z.object({
    id: text,
    name: text,
    description: note,
    category: note,
    kind: z.literal('select'),
    current: text,
    options: z
      .array(choiceSchema)
      .default([])
      .transform(uniqueBy((c) => c.value))
  }),
  z.object({
    id: text,
    name: text,
    description: note,
    category: note,
    kind: z.literal('boolean'),
    current: z.boolean()
  })
])

export const modeSchema = z.object({
  current: text,
  available: z
    .array(z.object({ id: text, name: text, description: note }))
    .default([])
    .transform(uniqueBy((m) => m.id))
})

export const commandSchema = z.object({ name: text, description: text.default(''), hint: note })

export const usageSchema = z.object({
  used: z.number(),
  size: z.number(),
  cost: z.object({ amount: z.number(), currency: text }).nullish()
})

export const planEntrySchema = z.object({
  content: text,
  status: z.enum(['pending', 'in_progress', 'completed']).catch('pending'),
  priority: z.enum(['high', 'medium', 'low']).catch('medium')
})

export const sessionSchema = z.object({
  agent: z.object({ name: text, version: text.nullish() }).nullish(),
  title: note,
  toolsConnected: z.boolean().default(true),
  modes: modeSchema.nullish(),
  config: z
    .array(settingSchema)
    .default([])
    .transform(uniqueBy((c) => c.id)),
  commands: z
    .array(commandSchema)
    .default([])
    .transform(uniqueBy((c) => c.name)),
  usage: usageSchema.nullish()
})

export const threadStateSchema = z.object({
  session: sessionSchema.default(() => sessionSchema.parse({})),
  plan: z.array(planEntrySchema).nullish()
})

export type Mode = z.infer<typeof modeSchema>
export type Command = z.infer<typeof commandSchema>
export type Usage = z.infer<typeof usageSchema>
export type SessionState = z.infer<typeof sessionSchema>
export type ThreadState = z.infer<typeof threadStateSchema>

export const emptyThreadState = (): ThreadState => threadStateSchema.parse({})

/** Parse a state document; a malformed one is an error (a bug in the host, shown rather than hidden). */
export const parseThreadState = (value: unknown): ThreadState => threadStateSchema.parse(value)

// ── thread snapshot ────────────────────────────────────────────────────────────────────────────────────────

const interruptSchema: z.ZodType<Interrupt> = z
  .object({
    id: text,
    reason: text,
    message: z.string().optional(),
    toolCallId: z.string().optional(),
    responseSchema: z.record(z.string(), z.any()).optional(),
    expiresAt: z.string().optional(),
    metadata: z.record(z.string(), z.any()).optional()
  })
  .loose() as unknown as z.ZodType<Interrupt>

export const snapshotSchema = z.object({
  threadId: text,
  agentId: text,
  running: z.boolean().default(false),
  state: threadStateSchema.default(() => emptyThreadState()),
  messages: z.array(z.record(z.string(), z.any())).default([]),
  interrupts: z
    .array(interruptSchema)
    .default([])
    .transform(uniqueBy((i) => i.id))
})
export type ThreadSnapshot = z.infer<typeof snapshotSchema>

// ── permission interrupts: the options live in the interrupt's metadata ────────────────────────────────────

export const permissionOptionSchema = z.object({
  optionId: text,
  name: text,
  kind: text
})
export type PermissionOption = z.infer<typeof permissionOptionSchema>

const permissionMetaSchema = z.object({
  'vos:options': z
    .array(permissionOptionSchema)
    .default([])
    .transform(uniqueBy((o) => o.optionId)),
  'vos:tool': z.object({ title: text.optional(), kind: z.string().nullish() }).optional()
})

export interface Permission {
  interruptId: string
  toolCallId?: string
  title: string
  options: PermissionOption[]
}

/** The permission a `tool_call` interrupt asks for. Unknown reasons and malformed metadata still produce a prompt. */
export function permissionOf(interrupt: Interrupt): Permission {
  const meta = permissionMetaSchema.safeParse(interrupt.metadata ?? {})
  const parsed = meta.success ? meta.data : { 'vos:options': [], 'vos:tool': undefined }
  return {
    interruptId: interrupt.id,
    toolCallId: interrupt.toolCallId,
    title: parsed['vos:tool']?.title || interrupt.message || 'run a tool',
    options: parsed['vos:options']
  }
}

// ── the `vos.tool` custom event ────────────────────────────────────────────────────────────────────────

export const TOOL_EVENT = 'vos.tool'
export const toolEventSchema = z.object({
  toolCallId: text,
  title: text.optional(),
  kind: z.string().nullish(),
  status: z.enum(['pending', 'in_progress', 'completed', 'failed'])
})
export type ToolEvent = z.infer<typeof toolEventSchema>
export type ToolStatus = ToolEvent['status']

// ── frames ─────────────────────────────────────────────────────────────────────────────────────────────────

/** Validate one SSE frame against the AG-UI event schemas. */
export function parseEvent(data: string): z.infer<typeof EventSchemas> {
  return EventSchemas.parse(JSON.parse(data))
}

export const errorBodySchema = z.object({ error: text })

/** The body of a run request (`POST /agui`). */
export function runInput(input: {
  threadId: string
  runId: string
  agentId: string
  messages?: RunAgentInput['messages']
  resume?: ResumeEntry[]
}): RunAgentInput {
  return {
    threadId: input.threadId,
    runId: input.runId,
    state: {},
    messages: input.messages ?? [],
    tools: [],
    context: [],
    forwardedProps: { agentId: input.agentId },
    ...(input.resume ? { resume: input.resume } : {})
  }
}
