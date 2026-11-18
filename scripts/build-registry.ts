#!/usr/bin/env bun
/**
 * Build this repo's app registry: every app module under apps/ (a package with
 * src/manifest.ts) → dist/registry/<id>/<version>/{server.js,client.js,manifest.json}
 * plus dist/registry/registry.json. The server installs from it and serves it
 * at /registry, so this repo is the first AgentOS registry.
 *
 *   bun run build:apps            build every app
 *   bun run build:apps calculator build one (others keep their last build)
 */
import { existsSync, readdirSync } from 'node:fs'
import { join, resolve } from 'node:path'
import { buildRegistry } from '@agentos/sdk/build'

const root = resolve(import.meta.dir, '..')
const only = process.argv.slice(2)
const appDirs = readdirSync(join(root, 'apps'))
  .map((d) => join(root, 'apps', d))
  .filter((d) => existsSync(join(d, 'src/manifest.ts')))
  .filter((d) => only.length === 0 || only.some((id) => d.endsWith(`/${id}`)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No app modules under apps/')
  process.exit(1)
}

const started = performance.now()
const registry = await buildRegistry({ name: 'agentos', appDirs, outDir: join(root, 'dist/registry'), merge: only.length > 0 })
for (const app of registry.apps.filter((a) => appDirs.some((d) => d.endsWith(`/${a.manifest.id}`)))) {
  console.log(`✓ ${app.manifest.id} ${app.manifest.version}  server ${app.server.sha256.slice(0, 12)}  client ${app.client.sha256.slice(0, 12)}`)
}
console.log(`registry "${registry.name}" → dist/registry/registry.json (${Math.round(performance.now() - started)}ms)`)
