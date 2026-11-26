#!/usr/bin/env bun
/**
 * Build the apps under apps/ into app refs.
 *
 *   bun run build:apps [ids…] [--out <dir>] [--root <dir>]...
 *
 * An app is a Vite project: an `openrpc.json` (its contract and only descriptor) and a `vite.config.ts` using the vos
 * plugin (`packages/vite-plugin`). Its own `bun run build` writes the whole app ref into its `dist/`: the backend as ONE
 * Luau file (`app.luau`, bundled by darklua with types and comments stripped), the page's static files, the SQL migrations
 * and `app-ref.json` ({ manifest, files: [{ path, url, sha256 }] }, file URLs relative to the app ref). This script runs
 * that build for every app and copies the folder under <out>/<id>/<version>/ (default out: dist/apps), then writes one
 * <out>/registry.json listing every app's app ref: { "apps": ["calculator/0.1.0/app-ref.json", ...] }.
 *
 * `--root <dir>` also builds the apps under <dir>/* (the e2e harness builds its test-only apps that way).
 *
 * The host's default registry is dist/apps/registry.json over file://;
 * with `dev_reload` it rebuilds an app's VM when its files change, so `bun run build:apps` is the dev loop.
 */
import { DefinitionError, parseDefinition } from '../packages/vite-plugin/src/definition.ts'
import { AppError, parseApp } from '../packages/vite-plugin-v2/src/app.ts'
import { spawnSync } from 'node:child_process'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { basename, join, relative, resolve } from 'node:path'
import { root } from './luau'

const args = process.argv.slice(2)
const outFlag = args.indexOf('--out')
const outDir = resolve(root, outFlag >= 0 ? (args[outFlag + 1] ?? '') : 'dist/apps')
const extraRoots = args.flatMap((a, i) => (a === '--root' ? [resolve(root, args[i + 1] ?? '')] : []))
const only = args.filter((a, i) => !a.startsWith('--') && args[i - 1] !== '--out' && args[i - 1] !== '--root')

/** An app is a Vite project: its contract, `openrpc.json`, is its only descriptor. */
const isApp = (dir: string): boolean => existsSync(join(dir, 'openrpc.json'))

/**
 * The id and version of an app, read with its plugin's own parser so that they are the validated ones (an id and a
 * version become directory names under the output folder): a v2 app's from its `package.json` (its contract has no
 * `x-app`), a v1 app's from `x-app.id` and `info.version` of its contract.
 */
function appIdentity(dir: string): { id: string; version: string } {
  try {
    const contract = JSON.parse(readFileSync(join(dir, 'openrpc.json'), 'utf8')) as Record<string, unknown>
    if (contract['x-app'] === undefined) {
      const app = parseApp(readFileSync(join(dir, 'package.json'), 'utf8'), `${relative(root, dir)}/package.json`)
      return { id: app.id, version: app.version }
    }
    const definition = parseDefinition(readFileSync(join(dir, 'openrpc.json'), 'utf8'), `${relative(root, dir)}/openrpc.json`)
    return { id: definition.app.id, version: definition.version }
  } catch (error) {
    if (!(error instanceof DefinitionError) && !(error instanceof AppError)) throw error
    console.error(`✗ ${error.message}`)
    process.exit(1)
  }
}

/**
 * Builds an app: `bun run build` in the project writes the whole app ref into its `dist/` (the backend bundled, the page's
 * static files, `app-ref.json` with a sha256 per file), and that folder is what goes under `<out>/<id>/<version>/`.
 */
function buildApp(dir: string, target: string, id: string): string[] | undefined {
  const result = spawnSync('bun', ['run', 'build'], { cwd: dir, encoding: 'utf8' })
  const dist = join(dir, 'dist')
  if (result.status !== 0 || !existsSync(join(dist, 'app-ref.json'))) {
    console.error(`✗ ${id}: the build failed (bun run build in ${relative(root, dir)}, expecting dist/app-ref.json):\n${result.stdout}${result.stderr}`)
    return undefined
  }
  // the old ref goes only once the new one is built: a watching host never sees a build that failed
  rmSync(target, { recursive: true, force: true })
  mkdirSync(target, { recursive: true })
  cpSync(dist, target, { recursive: true })
  return (JSON.parse(readFileSync(join(dist, 'app-ref.json'), 'utf8')) as { files: unknown[] }).files.map(String)
}

const appDirs = [join(root, 'apps'), ...extraRoots]
  .flatMap((parent) => readdirSync(parent).map((d) => join(parent, d)))
  .filter(isApp)
  .filter((d) => only.length === 0 || only.includes(basename(d)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No apps (apps/*/openrpc.json)')
  process.exit(1)
}

const started = performance.now()
const registry: string[] = []
const builtIds = new Set<string>()
const failedIds = new Set<string>()
for (const dir of appDirs) {
  const { id, version } = appIdentity(dir)
  const files = buildApp(dir, join(outDir, id, version), id)
  if (files === undefined) {
    failedIds.add(id)
    continue
  }
  registry.push(`${id}/${version}/app-ref.json`)
  builtIds.add(id)
  console.log(`✓ ${id} ${version}  ${files.length} files`)
}
// an app whose build failed keeps its last good ref, and a build of some apps keeps the others: only a full build drops
// an app that is gone
const registryFile = join(outDir, 'registry.json')
if (existsSync(registryFile)) {
  const before = (JSON.parse(readFileSync(registryFile, 'utf8')) as { apps: string[] }).apps
  const kept = (id: string): boolean => !builtIds.has(id) && (only.length > 0 || failedIds.has(id))
  registry.push(...before.filter((ref) => kept(ref.split('/')[0] ?? '') && existsSync(join(outDir, ref))))
}
registry.sort()
writeFileSync(registryFile, JSON.stringify({ apps: registry }, null, 2) + '\n')
console.log(`built → ${relative(root, outDir) || '.'}/registry.json (${Math.round(performance.now() - started)}ms)`)
if (failedIds.size > 0) {
  console.error(`✗ not built: ${[...failedIds].join(', ')}`)
  process.exit(1)
}
