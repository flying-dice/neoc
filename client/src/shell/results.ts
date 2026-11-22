/**
 * What the host answers to the shell's own calls, parsed with zod at the edge. A host of another version (or a bug in
 * one) can answer `{ "apps": null }`; a cast would carry that into the desktop and crash it on the first read. A
 * failed parse is an error the caller shows, never a throw out of a render.
 *
 * The shapes mirror `crates/vos-domain/src/api.rs`; the `satisfies` clauses make a drift from `hostApi.ts`
 * (and so from the generated types) a type error. Unknown fields are dropped, so a field the shell never reads
 * does not travel further than this module.
 */
import { uniqueBy } from '@vos/ui'
import { z } from 'zod'
import type {
  AppNotification,
  AppPreview,
  AppSummary,
  Catalog,
  DeviceRow,
  FileEntry,
  FileLocations,
  HostApi
} from './hostApi'

/** The host's answer was not what `method` promises. */
export class ResultError extends Error {
  constructor(
    readonly method: string,
    detail: string
  ) {
    super(`vos sent an unexpected answer to “${method}” (${detail}). Reload to update.`)
    this.name = 'ResultError'
  }
}

const text = z.string()

const appStateSchema = z.enum(['running', 'disabled', 'failed'])

const appSummarySchema = z.object({
  id: text,
  name: text,
  version: text,
  state: appStateSchema,
  message: text.optional(),
  icon: text.optional(),
  tint: text.optional(),
  description: text.optional(),
  window: z.object({ w: z.number(), h: z.number(), minW: z.number(), minH: z.number() }).optional(),
  source: text,
  permissions: z.array(text)
}) satisfies z.ZodType<AppSummary>

export const appListSchema = z.object({ apps: z.array(appSummarySchema) })

export const appPreviewSchema = z.object({
  url: text,
  id: text,
  name: text,
  version: text,
  icon: text.optional(),
  tint: text.optional(),
  description: text.optional(),
  source: text,
  installed: z.object({ version: text, state: appStateSchema }).optional(),
  /** The app ref's digest as previewed: `app.install` sends it back so only that ref installs. */
  sha256: z.string().regex(/^[0-9a-f]{64}$/i)
}) satisfies z.ZodType<AppPreview>

export const catalogSchema = z.object({
  apps: z.array(appPreviewSchema),
  // registry problems are keyed by their text in the page: the same problem twice must show once
  errors: z.array(text).transform(uniqueBy((error) => error))
}) satisfies z.ZodType<Catalog>

export const deviceListSchema = z.object({
  devices: z.array(
    z.object({
      id: text,
      name: text,
      createdAt: z.number(),
      lastSeenAt: z.number(),
      current: z.boolean()
    }) satisfies z.ZodType<DeviceRow>
  )
})

export const fileListSchema = z.object({
  entries: z.array(
    z.object({
      name: text,
      kind: z.enum(['file', 'dir', 'link', 'other']),
      size: z.number(),
      modifiedAt: z.number().optional(),
      link: z.boolean(),
      problem: text.optional()
    }) satisfies z.ZodType<FileEntry>
  ),
  truncated: z.boolean()
})

export const fileLocationsSchema = z.object({
  computer: z.boolean(),
  home: text.optional(),
  maxUploadBytes: z.number()
}) satisfies z.ZodType<FileLocations>

/** The params of `notification.deliver`. A message of another shape is dropped, not drawn. */
export const appNotificationSchema = z.object({
  app: text,
  title: text,
  body: text.optional()
}) satisfies z.ZodType<AppNotification>

/** Call a host method and parse its result; a result of another shape rejects with a `ResultError`. */
export async function callParsed<S extends z.ZodType>(
  api: HostApi,
  method: string,
  params: Record<string, unknown>,
  schema: S
): Promise<z.infer<S>> {
  const result = await api.call(method, params)
  const parsed = schema.safeParse(result)
  if (parsed.success) return parsed.data
  const issue = parsed.error.issues[0]
  const where = issue?.path.length ? `${issue.path.join('.')}: ` : ''
  throw new ResultError(method, `${where}${issue?.message ?? 'invalid'}`)
}
