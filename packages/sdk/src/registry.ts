/**
 * The registry format. A registry is any HTTP(S) location (or local
 * directory) serving `registry.json` plus the bundles it points at; this repo
 * builds the first one (`bun run build:apps` → dist/registry). Bundle URLs
 * are relative to registry.json. Hashes are verified on install.
 */
import { z } from 'zod'

const sha256 = z.string().regex(/^[0-9a-f]{64}$/)

export const manifestSchema = z.object({
  id: z.string().regex(/^[a-z][a-z0-9-]{1,31}$/),
  version: z.string().regex(/^\d+\.\d+\.\d+(-[0-9A-Za-z.-]+)?$/),
  name: z.string().min(1).max(60),
  icon: z.string().min(1),
  background: z.string().min(1).max(300),
  window: z
    .object({
      width: z.number().int().positive(),
      height: z.number().int().positive(),
      minWidth: z.number().int().positive().optional(),
      minHeight: z.number().int().positive().optional()
    })
    .optional(),
  dock: z.boolean().optional()
})

export const bundleRefSchema = z.object({ url: z.string().min(1), sha256 })

export const registryEntrySchema = z.object({
  manifest: manifestSchema,
  server: bundleRefSchema,
  client: bundleRefSchema
})

export const registrySchema = z.object({
  /** Short stable name, e.g. "agentos". */
  name: z.string().regex(/^[a-z][a-z0-9-]{1,40}$/),
  apps: z.array(registryEntrySchema)
})

export type RegistryEntry = z.infer<typeof registryEntrySchema>
export type Registry = z.infer<typeof registrySchema>
