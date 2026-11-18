/**
 * The app bundle descriptor (`bundle.json`). Every built app is a directory of
 * static files that can be hosted anywhere (this repo's `bundles/`, GitLab
 * Pages, any static host):
 *
 *   <id>/bundle.json                  latest version (URLs point into <version>/)
 *   <id>/<version>/bundle.json        that version (URLs relative to it)
 *   <id>/<version>/server.js          ESM for the Bun server
 *   <id>/<version>/client.js          ESM for the browser
 *
 * A server installs an app from the URL of a bundle.json in its known-apps
 * list. URLs inside are relative to the descriptor; the server checks every
 * file's sha256 before installing it.
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

export const bundleFileSchema = z.object({ url: z.string().min(1), sha256 })

export const bundleDescriptorSchema = z.object({
  manifest: manifestSchema,
  server: bundleFileSchema,
  client: bundleFileSchema
})

export type BundleFile = z.infer<typeof bundleFileSchema>
export type BundleDescriptor = z.infer<typeof bundleDescriptorSchema>

/**
 * A server's known apps (`known-apps.json`): the bundle.json URLs it offers to
 * install. Relative entries resolve against the file itself, so a list can
 * point at bundles on disk (this repo's `bundles/`) or on any static host.
 */
export const knownAppsSchema = z.object({ apps: z.array(z.string().min(1)) })
export type KnownAppsFile = z.infer<typeof knownAppsSchema>
