#!/usr/bin/env bun
/**
 * Build the Luau app packages under apps/ (every folder with an `app.json`) into app refs.
 *
 *   bun run build:apps [ids…] [--out <dir>] [--root <dir>]...
 *
 * For each app this writes, under <out>/<id>/<version>/ (default out: dist/apps):
 *   app-ref.json   { manifest, files: [{ path, url, sha256 }] }, file URLs relative to the app ref
 *   app.luau       the app as ONE Luau file: the entry (app.json `main`, app.luau) and the modules it
 *                  requires, bundled by darklua (.darklua.json), types and comments stripped; `vos:app` and `vos:ui`
 *                  stay runtime `require` calls, the host provides them (with types/generated/prelude.lua: `typeof`
 *                  and `utf8`, from lua/prelude.lua)
 *   <migrations>   the SQL migrations app.json lists, copied as they are
 * The ref's `manifest.main` names the bundled `.luau` file (the entry the host loads; the host takes any name, the
 * bundle keeps its source's name and is Luau, not Lua 5.1); the separate source modules are not shipped.
 * darklua is required (pinned, see scripts/luau.ts): the build fails with an install message without it.
 * `--root <dir>` also builds the packages under <dir>/* (the e2e harness builds a test-only app that way).
 * and one <out>/registry.json listing every app's app ref: { "apps": ["calculator/0.1.0/app-ref.json", ...] }.
 *
 * The host's default registry is dist/apps/registry.json over file://;
 * with `dev_reload` it rebuilds an app's VM when its files change, so `bun run build:apps` is the dev loop.
 *
 * An app that is a Vite project (an `openrpc.json`, its contract and only descriptor, and a `vite.config.ts` using the vos plugin:
 * `packages/vite-plugin`) is built by its own project: `bun run build` writes the whole app ref into its `dist/` (backend
 * bundled by the plugin, the client script, `app-ref.json`), and that folder is copied under <out>/<id>/<version>/. Every
 * other app (`app.json` + Luau) is bundled here.
 *
 * `app.json` is the package manifest (id, name, version, main, window, icon, tint, description,
 * requires, migrations; a `permissions` key from before apps lost their permissions is ignored); the build checks it
 * against the `app.manifest { ... }` the app declares in Luau, so
 * the two cannot disagree about the id, icon or tint.
 */
import { DefinitionError, parseDefinition } from '../packages/vite-plugin/src/definition.ts'
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { z } from 'zod'
import { requireDarklua, root, toLuau } from './luau'

const args = process.argv.slice(2)
const outFlag = args.indexOf('--out')
const outDir = resolve(root, outFlag >= 0 ? (args[outFlag + 1] ?? '') : 'dist/apps')
const extraRoots = args.flatMap((a, i) => (a === '--root' ? [resolve(root, args[i + 1] ?? '')] : []))
const only = args.filter(
  (a, i) => !a.startsWith('--') && i !== outFlag + 1 && args[i - 1] !== '--root'
)

const PackageManifest = z.object({
  id: z.string().regex(/^[a-z0-9][a-z0-9-]{2,39}$/),
  name: z.string().min(1).max(60),
  version: z.string().regex(/^\d+\.\d+\.\d+$/),
  description: z.string().optional(),
  main: z.string().default('app.luau'),
  icon: z.string().optional(),
  tint: z.string().optional(),
  requires: z.object({ host: z.string().optional() }).optional(),
  window: z.object({ w: z.number().int(), h: z.number().int(), minW: z.number().int(), minH: z.number().int() }).optional(),
  migrations: z.array(z.object({ version: z.number().int(), name: z.string(), file: z.string() })).default([])
})

/** The string literal assigned to `field` inside the app's `app.manifest { ... }` call. */
function declared(source: string, field: string): string | undefined {
  const block = source.match(/app\.manifest\s*\{([\s\S]*?)\n\}/)?.[1] ?? ''
  return block.match(new RegExp(`\\b${field}\\s*=\\s*"([^"]*)"`))?.[1]
}

/** An app that is a Vite project has its contract, `openrpc.json`, as its only descriptor (no `app.json`). */
const isViteApp = (dir: string): boolean => existsSync(join(dir, 'openrpc.json'))

/**
 * The id and version of a Vite project: `x-app.id` and `info.version` of its contract, read with the plugin's own parser so that
 * they are the validated ones (an id and a version become directory names under the output folder).
 */
function viteAppIdentity(dir: string): { id: string; version: string } {
  try {
    const definition = parseDefinition(readFileSync(join(dir, 'openrpc.json'), 'utf8'), `${relative(root, dir)}/openrpc.json`)
    return { id: definition.app.id, version: definition.version }
  } catch (error) {
    if (!(error instanceof DefinitionError)) throw error
    console.error(`✗ ${error.message}`)
    process.exit(1)
  }
}

/**
 * Builds an app that is a Vite project (its `openrpc.json` and `vite.config.ts`, with the vos plugin): `bun run build` in the
 * project writes the whole app ref into its `dist/` (the backend bundled, the client script, `app-ref.json` with a sha256
 * per file), and that folder is what goes under `<out>/<id>/<version>/`.
 */
function buildViteApp(dir: string, target: string, id: string): string[] {
  const result = spawnSync('bun', ['run', 'build'], { cwd: dir, encoding: 'utf8' })
  const dist = join(dir, 'dist')
  if (result.status !== 0 || !existsSync(join(dist, 'app-ref.json'))) {
    console.error(`✗ ${id}: the build failed (bun run build in ${relative(root, dir)}, expecting dist/app-ref.json):\n${result.stdout}${result.stderr}`)
    process.exit(1)
  }
  cpSync(dist, target, { recursive: true })
  return (JSON.parse(readFileSync(join(dist, 'app-ref.json'), 'utf8')) as { files: unknown[] }).files.map(String)
}

const appDirs = [join(root, 'apps'), ...extraRoots]
  .flatMap((parent) => readdirSync(parent).map((d) => join(parent, d)))
  .filter((d) => existsSync(join(d, 'app.json')) || isViteApp(d))
  .filter((d) => only.length === 0 || only.some((id) => d.endsWith(`/${id}`)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No app packages (apps/*/app.json)')
  process.exit(1)
}

const darklua = requireDarklua()
const started = performance.now()
const registry: string[] = []
// TODO: clean-code - 0.4 - SRP: this loop parses the manifest, checks it against the Luau source, bundles, copies and hashes migrations, writes the app ref and registers it; extract `checkManifestAgainstLua`, `buildApp` and `writeAppRef`.
for (const dir of appDirs) {
  if (isViteApp(dir)) {
    const { id, version } = viteAppIdentity(dir)
    const target = join(outDir, id, version)
    rmSync(target, { recursive: true, force: true })
    mkdirSync(target, { recursive: true })
    const files = buildViteApp(dir, target, id)
    registry.push(`${id}/${version}/app-ref.json`)
    console.log(`✓ ${id} ${version}  ${files.length} files (vite)`)
    continue
  }
  const manifest = PackageManifest.parse(JSON.parse(readFileSync(join(dir, 'app.json'), 'utf8')))
  const source = readFileSync(join(dir, manifest.main), 'utf8')
  for (const field of ['id', 'icon', 'tint'] as const) {
    const inLua = declared(source, field)
    if (inLua !== undefined && inLua !== manifest[field]) {
      console.error(`✗ ${manifest.id}: app.json ${field} "${manifest[field]}" differs from app.manifest ${field} "${inLua}"`)
      process.exit(1)
    }
  }
  const target = join(outDir, manifest.id, manifest.version)
  rmSync(target, { recursive: true, force: true })
  mkdirSync(target, { recursive: true })
  const sha256 = (bytes: string | Buffer): string => createHash('sha256').update(bytes).digest('hex')
  const entry = manifest.main
  let bundle: string
  try {
    bundle = toLuau(darklua, join(dir, manifest.main))
  } catch (error) {
    console.error(`✗ ${manifest.id}: darklua could not build ${manifest.main}:\n${(error as Error).message}`)
    process.exit(1)
  }
  mkdirSync(dirname(join(target, entry)), { recursive: true })
  writeFileSync(join(target, entry), bundle)
  const refs = [{ path: entry, url: entry, sha256: sha256(bundle) }]
  if (manifest.client) {
    // the client is built by the app's own package (`bun run build` in its folder writes dist/client.js): any
    // framework, one classic script that defines `vosApp.mount(el, { rpc, host })`
    const built = buildClient(dir, manifest.id)
    mkdirSync(dirname(join(target, manifest.client)), { recursive: true })
    writeFileSync(join(target, manifest.client), built)
    refs.push({ path: manifest.client, url: manifest.client, sha256: sha256(built) })
  }
  for (const { file } of manifest.migrations) {
    mkdirSync(dirname(join(target, file)), { recursive: true })
    cpSync(join(dir, file), join(target, file))
    refs.push({ path: file, url: file, sha256: sha256(readFileSync(join(dir, file))) })
  }
  const appRef = {
    manifest: {
      id: manifest.id,
      name: manifest.name,
      version: manifest.version,
      main: entry,
      migrations: manifest.migrations,
      ...(manifest.icon ? { icon: manifest.icon } : {}),
      ...(manifest.tint ? { tint: manifest.tint } : {}),
      ...(manifest.description ? { description: manifest.description } : {}),
      ...(manifest.requires ? { requires: manifest.requires } : {}),
      ...(manifest.window ? { window: manifest.window } : {})
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
