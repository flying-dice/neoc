#!/usr/bin/env bun
/**
 * Build the Luau app packages under apps/ (every folder with an `app.json`) into app refs.
 *
 *   bun run build:apps [ids…] [--out <dir>] [--root <dir>]...
 *
 * For each app this writes, under <out>/<id>/<version>/ (default out: dist/apps):
 *   app-ref.json   { manifest, files: [{ path, url, sha256 }] }, file URLs relative to the app ref
 *   app.lua        the app as ONE Lua 5.1 file: the Luau entry (app.json `main`, app.luau) and the modules it
 *                  requires, bundled and stripped by darklua (.darklua.json); `@agentos/app` and `@agentos/ui`
 *                  stay runtime `require` calls, the host provides them (with lua/prelude.luau, the Lua 5.1
 *                  polyfills for the Luau standard library, built to types/generated/prelude.lua)
 *   <migrations>   the SQL migrations app.json lists, copied as they are
 * The ref's `manifest.main` names the bundled `.lua` file (the entry the host loads); the Luau sources are not shipped.
 * darklua is required (pinned, see scripts/lua51.ts): the build fails with an install message without it.
 * `--root <dir>` also builds the packages under <dir>/* (the e2e harness builds a test-only app that way).
 * and one <out>/registry.json listing every app's app ref: { "apps": ["calculator/0.1.0/app-ref.json", ...] }.
 *
 * The host's default registry is dist/apps/registry.json over file:// (see docs/architecture/host-stack.md);
 * with `dev_reload` it rebuilds an app's VM when its files change, so `bun run build:apps` is the dev loop.
 *
 * `app.json` is the package manifest (id, name, version, main, icon, tint, description,
 * requires, migrations; a `permissions` key from before apps lost their permissions is ignored); the build checks it against the `app.manifest { ... }` the app declares in Luau, so
 * the two cannot disagree about the id, icon or tint.
 */
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { dirname, join, relative, resolve } from 'node:path'
import { z } from 'zod'
import { requireDarklua, root, toLua51 } from './lua51'

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
  migrations: z.array(z.object({ version: z.number().int(), name: z.string(), file: z.string() })).default([])
})

/** The string literal assigned to `field` inside the app's `app.manifest { ... }` call. */
function declared(source: string, field: string): string | undefined {
  const block = source.match(/app\.manifest\s*\{([\s\S]*?)\n\}/)?.[1] ?? ''
  return block.match(new RegExp(`\\b${field}\\s*=\\s*"([^"]*)"`))?.[1]
}

const appDirs = [join(root, 'apps'), ...extraRoots]
  .flatMap((parent) => readdirSync(parent).map((d) => join(parent, d)))
  .filter((d) => existsSync(join(d, 'app.json')))
  .filter((d) => only.length === 0 || only.some((id) => d.endsWith(`/${id}`)))

if (appDirs.length === 0) {
  console.error(only.length ? `No app named ${only.join(', ')} under apps/` : 'No app packages (apps/*/app.json)')
  process.exit(1)
}

const darklua = requireDarklua()
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
  const sha256 = (bytes: string | Buffer): string => createHash('sha256').update(bytes).digest('hex')
  const entry = manifest.main.replace(/\.luau$/, '.lua')
  let bundle: string
  try {
    bundle = toLua51(darklua, join(dir, manifest.main))
  } catch (error) {
    console.error(`✗ ${manifest.id}: darklua could not build ${manifest.main}:\n${(error as Error).message}`)
    process.exit(1)
  }
  mkdirSync(dirname(join(target, entry)), { recursive: true })
  writeFileSync(join(target, entry), bundle)
  const refs = [{ path: entry, url: entry, sha256: sha256(bundle) }]
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
