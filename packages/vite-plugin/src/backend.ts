/**
 * The backend half of the build: the Luau entry bundled into one file with darklua (relative `require`s inlined, types
 * and comments stripped), and the app ref the host installs: the manifest from `app.json` plus every file with its sha256.
 * The output goes next to the client script, so the build's output folder is one installable app.
 */
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { cpSync, existsSync, mkdirSync, mkdtempSync, readdirSync, readFileSync, renameSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import { inside } from './paths.ts'
import type { Definition } from './definition.ts'

/** The darklua release the bundles are pinned to (the host's own build uses the same one). */
export const DARKLUA_VERSION = '0.18.0'

// `fileURLToPath`, not `.pathname`: a folder name with a space or any other character a URL percent-encodes stays itself
const DARKLUA_CONFIG = fileURLToPath(new URL('../darklua.json', import.meta.url))

function findDarklua(): string | undefined {
  const which = spawnSync('which', ['darklua'], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  const cargo = join(homedir(), '.cargo/bin/darklua')
  return existsSync(cargo) ? cargo : undefined
}

/** The pinned darklua, or an error that says how to install it. */
export function requireDarklua(): string {
  const darklua = findDarklua()
  if (!darklua) {
    throw new Error(
      `darklua not found. It bundles the Luau backend; install the pinned version:\n  cargo install darklua --version ${DARKLUA_VERSION} --locked`
    )
  }
  const version = spawnSync(darklua, ['--version'], { encoding: 'utf8' }).stdout.trim()
  if (version !== `darklua ${DARKLUA_VERSION}`) {
    throw new Error(`${darklua} is "${version}", the build is pinned to darklua ${DARKLUA_VERSION}:\n  cargo install darklua --version ${DARKLUA_VERSION} --locked --force`)
  }
  return darklua
}

/** The bundled Luau text of `entry`, with the relative requires inlined. */
export function bundleLuau(entry: string): string {
  const darklua = requireDarklua()
  const scratch = mkdtempSync(join(tmpdir(), 'vos-darklua-'))
  try {
    const output = join(scratch, 'out.luau')
    const run = spawnSync(darklua, ['process', '--config', DARKLUA_CONFIG, entry, output], {
      cwd: dirname(entry),
      encoding: 'utf8'
    })
    if (run.status !== 0) throw new Error(`darklua could not bundle ${entry}:\n${`${run.stderr}${run.stdout}`.trim()}`)
    return readFileSync(output, 'utf8')
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

const sha256 = (bytes: string | Buffer): string => createHash('sha256').update(bytes).digest('hex')

/** Every file under `dir`, as paths relative to it with `/`, in a stable order. */
export function filesUnder(dir: string, prefix = ''): string[] {
  if (!existsSync(join(dir, prefix))) return []
  return readdirSync(join(dir, prefix), { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : 1))
    .flatMap((entry) => {
      const path = prefix === '' ? entry.name : `${prefix}/${entry.name}`
      return entry.isDirectory() ? filesUnder(dir, path) : [path]
    })
}

/**
 * Write `app.luau` and `app-ref.json` into `outDir`, which already holds the client script: the build's staging folder, not yet
 * `dist/`. The manifest is the contract's (`info.title` and `info.version`, and `x-app`); every file the app ref lists is next to
 * it, with a relative URL, so the folder installs from `file://`, `http://` or `https://` as it is. Every file a migration names
 * is read from inside the project and written to inside `outDir`, whatever the contract says.
 */
export function writeApp(project: string, outDir: string, definition: Definition, entry: string, client: string): void {
  mkdirSync(outDir, { recursive: true })
  const bundle = bundleLuau(join(project, entry))
  const clientBytes = readFileSync(join(outDir, client))
  const files = [
    { path: 'app.luau', url: 'app.luau', sha256: sha256(bundle) },
    { path: client, url: client, sha256: sha256(clientBytes) }
  ]
  const meta = definition.app
  for (const [index, { file }] of (meta.migrations ?? []).entries()) {
    const from = inside(project, file)
    if (!existsSync(from) || !statSync(from).isFile()) {
      throw new Error(`x-app.migrations[${index}].file \`${file}\` is not a file of the project`)
    }
    const to = inside(outDir, file)
    mkdirSync(dirname(to), { recursive: true })
    cpSync(from, to)
    files.push({ path: file, url: file, sha256: sha256(readFileSync(from)) })
  }
  const appRef = {
    manifest: {
      id: meta.id,
      name: definition.title,
      version: definition.version,
      main: 'app.luau',
      client,
      migrations: meta.migrations ?? [],
      ...(meta.icon ? { icon: meta.icon } : {}),
      ...(meta.tint ? { tint: meta.tint } : {}),
      ...(meta.description ? { description: meta.description } : {}),
      ...(meta.window ? { window: meta.window } : {}),
      ...(meta.requires ? { requires: meta.requires } : {}),
      // what the app's frame may load and ask for (MCP Apps' `_meta.ui`): the host builds the frame's policy and `allow` from it
      ...(meta.csp || meta.permissions ? { ui: { ...(meta.csp ? { csp: meta.csp } : {}), ...(meta.permissions ? { permissions: meta.permissions } : {}) } } : {})
    },
    files
  }
  writeFileSync(join(outDir, 'app.luau'), bundle)
  writeFileSync(join(outDir, 'app-ref.json'), JSON.stringify(appRef, null, 2) + '\n')
}

/**
 * Put a finished build in place: every file of `staging` moves into `dist` (the files of the app ref last, `app-ref.json` the very
 * last, so a reader that follows it finds everything it lists), then what the new build no longer has is removed. `dist/` is
 * only touched after every step of the build has succeeded, and its folder stays the same folder (a watcher on it keeps watching).
 */
export function promote(staging: string, dist: string): void {
  const fresh = filesUnder(staging)
  const last = ['app.luau', 'app-ref.json']
  const order = [...fresh.filter((f) => !last.includes(f)), ...last.filter((f) => fresh.includes(f))]
  mkdirSync(dist, { recursive: true })
  for (const file of order) {
    const to = inside(dist, file)
    mkdirSync(dirname(to), { recursive: true })
    renameSync(inside(staging, file), to)
  }
  const keep = new Set(fresh)
  for (const stale of filesUnder(dist)) if (!keep.has(stale)) rmSync(inside(dist, stale), { force: true })
  removeEmptyDirs(dist)
}

function removeEmptyDirs(dir: string): void {
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    if (!entry.isDirectory()) continue
    const path = join(dir, entry.name)
    removeEmptyDirs(path)
    if (readdirSync(path).length === 0) rmSync(path, { recursive: true })
  }
}
