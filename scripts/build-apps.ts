#!/usr/bin/env bun
/**
 * Build the Luau app packages under apps/ (every folder with an `app.json`) into app refs.
 *
 *   bun run build:apps [ids…] [--out <dir>]
 *
 * For each app this writes, under <out>/<id>/<version>/ (default out: dist/apps):
 *   app-ref.json   { manifest, files: [{ path, url, sha256 }] }, file URLs relative to the app ref
 *   <the files>    app.luau, its modules and SQL migrations, copied as they are
 * and one <out>/registry.json listing every app's app ref: { "apps": ["calculator/0.1.0/app-ref.json", ...] }.
 *
 * The host's default registry is dist/apps/registry.json over file:// (see docs/architecture/host-stack.md);
 * with `dev_reload` it rebuilds an app's VM when its files change, so `bun run build:apps` is the dev loop.
 *
 * `app.json` is the package manifest (id, name, version, main, permissions, icon, tint, description,
 * requires, migrations); the build checks it against the `app.manifest { ... }` the app declares in Luau, so
 * the two cannot disagree about the id, icon or tint.
 */
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join, relative, resolve } from 'node:path'
import { z } from 'zod'

const root = resolve(import.meta.dir, '..')
const args = process.argv.slice(2)
const outFlag = args.indexOf('--out')
const outDir = resolve(root, outFlag >= 0 ? (args[outFlag + 1] ?? '') : 'dist/apps')
const only = args.filter((a, i) => !a.startsWith('--') && i !== outFlag + 1)

const Capability = z.enum(['data', 'notify', 'shell'])
const PackageManifest = z.object({
  id: z.string().regex(/^[a-z0-9][a-z0-9-]{2,39}$/),
  name: z.string().min(1).max(60),
  version: z.string().regex(/^\d+\.\d+\.\d+$/),
  description: z.string().optional(),
  main: z.string().default('app.luau'),
  icon: z.string().optional(),
  tint: z.string().optional(),
  permissions: z.array(Capability).default([]),
  requires: z.object({ host: z.string().optional() }).optional(),
  migrations: z.array(z.object({ version: z.number().int(), name: z.string(), file: z.string() })).default([])
})

/** Files that are part of the package: Luau sources (not tests) and the migrations app.json lists. */
function packageFiles(dir: string, migrations: string[]): string[] {
  const found: string[] = []
  const walk = (current: string): void => {
    for (const entry of readdirSync(current)) {
      const path = join(current, entry)
      const rel = relative(dir, path)
      if (statSync(path).isDirectory()) {
        if (!['tests', 'e2e', 'negative', 'node_modules'].includes(entry)) walk(path)
      } else if (rel.endsWith('.luau')) found.push(rel)
    }
  }
  walk(dir)
  return [...found, ...migrations].sort()
}

/** The string literal assigned to `field` inside the app's `app.manifest { ... }` call. */
function declared(source: string, field: string): string | undefined {
  const block = source.match(/app\.manifest\s*\{([\s\S]*?)\n\}/)?.[1] ?? ''
  return block.match(new RegExp(`\\b${field}\\s*=\\s*"([^"]*)"`))?.[1]
}

const appDirs = readdirSync(join(root, 'apps'))
  .map((d) => join(root, 'apps', d))
  .filter((d) => existsSync(join(d, 'app.json')))
  .filter((d) => only.length === 0 || only.some((id) => d.endsWith(`/${id}`)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No app packages (apps/*/app.json)')
  process.exit(1)
}

const started = performance.now()
const registry: string[] = []
for (const dir of appDirs) {
  const manifest = PackageManifest.parse(JSON.parse(readFileSync(join(dir, 'app.json'), 'utf8')))
  const source = readFileSync(join(dir, manifest.main), 'utf8')
  for (const field of ['id', 'icon', 'tint'] as const) {
    const inLuau = declared(source, field)
    if (inLuau !== undefined && inLuau !== manifest[field]) {
      console.error(`✗ ${manifest.id}: app.json ${field} "${manifest[field]}" differs from app.manifest ${field} "${inLuau}"`)
      process.exit(1)
    }
  }
  const target = join(outDir, manifest.id, manifest.version)
  rmSync(target, { recursive: true, force: true })
  mkdirSync(target, { recursive: true })
  const files = packageFiles(dir, manifest.migrations.map((m) => m.file))
  const refs = files.map((path) => {
    const bytes = readFileSync(join(dir, path))
    mkdirSync(join(target, path, '..'), { recursive: true })
    cpSync(join(dir, path), join(target, path))
    return { path, url: path, sha256: createHash('sha256').update(bytes).digest('hex') }
  })
  const appRef = {
    manifest: {
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      main: manifest.main,
      permissions: manifest.permissions.map((kind) => ({ kind })),
      migrations: manifest.migrations,
      ...(manifest.icon ? { icon: manifest.icon } : {}),
      ...(manifest.tint ? { tint: manifest.tint } : {}),
      ...(manifest.description ? { description: manifest.description } : {}),
      ...(manifest.requires ? { requires: manifest.requires } : {})
    },
    files: refs
  }
  writeFileSync(join(target, 'app-ref.json'), JSON.stringify(appRef, null, 2) + '\n')
  registry.push(`${manifest.id}/${manifest.version}/app-ref.json`)
  console.log(`✓ ${manifest.id} ${manifest.version}  ${refs.length} files`)
}
registry.sort()
writeFileSync(join(outDir, 'registry.json'), JSON.stringify({ apps: registry }, null, 2) + '\n')
console.log(`built → ${relative(root, outDir) || '.'}/registry.json (${Math.round(performance.now() - started)}ms)`)
