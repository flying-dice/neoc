#!/usr/bin/env bun
/**
 * Build the app modules under apps/ (every package with src/manifest.ts) into
 * static bundle directories (<id>/bundle.json, <id>/<version>/{bundle.json,server.js,client.js}).
 *
 *   bun run build:apps [ids…]     dev build, mutable → dist/bundles/ (gitignored). The dev server
 *                                 serves it through AGENTOS_APPS_BASE and hot-reloads installed apps.
 *   bun run release:apps [ids…]   release build, immutable → bundles/ (committed). The known-apps
 *                                 list's base URL points at it on GitHub. A published version can't
 *                                 change: bump `version` in the app's manifest to release a change.
 *                                 Also adds <id>/bundle.json to apps/server/known-apps.json if missing.
 */
import { existsSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { knownAppsSchema } from '@agentos/sdk/bundle'
import { buildAppBundle, PublishedVersionError } from '@agentos/sdk/build'

const root = resolve(import.meta.dir, '..')
const release = process.argv.includes('--release')
const only = process.argv.slice(2).filter((a) => a !== '--release')
const outDir = join(root, release ? 'bundles' : 'dist/bundles')
const knownAppsFile = join(root, 'apps/server/known-apps.json')

const appDirs = readdirSync(join(root, 'apps'))
  .map((d) => join(root, 'apps', d))
  .filter((d) => existsSync(join(d, 'src/manifest.ts')))
  .filter((d) => only.length === 0 || only.some((id) => d.endsWith(`/${id}`)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No app modules under apps/')
  process.exit(1)
}

/** Add `<id>/bundle.json` to the known-apps list when it isn't there (an absolute entry for the id counts). */
async function ensureKnown(ids: string[]): Promise<string[]> {
  const list = knownAppsSchema.parse(await Bun.file(knownAppsFile).json())
  const added: string[] = []
  for (const id of ids) {
    const entry = `${id}/bundle.json`
    if (list.apps.includes(entry)) continue
    list.apps.push(entry)
    added.push(entry)
  }
  if (added.length) await Bun.write(knownAppsFile, JSON.stringify(list, null, 2) + '\n')
  return added
}

const started = performance.now()
const ids: string[] = []
for (const appDir of appDirs) {
  try {
    const { manifest, server, client } = await buildAppBundle({ appDir, outDir, immutable: release })
    ids.push(manifest.id)
    console.log(`✓ ${manifest.id} ${manifest.version}  server ${server.sha256.slice(0, 12)}  client ${client.sha256.slice(0, 12)}`)
  } catch (error) {
    if (!(error instanceof PublishedVersionError)) throw error
    console.error(`✗ ${error.message}`)
    process.exit(1)
  }
}
const added = release ? await ensureKnown(ids) : []
for (const entry of added) console.log(`+ ${entry} added to apps/server/known-apps.json`)
console.log(`${release ? 'released' : 'built'} → ${release ? 'bundles' : 'dist/bundles'}/ (${Math.round(performance.now() - started)}ms)`)
