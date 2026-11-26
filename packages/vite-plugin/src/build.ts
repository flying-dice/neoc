/**
 * The build output: the backend bundled into one Luau chunk (darklua: relative `require`s inlined, types and comments
 * stripped) and the app ref the host installs (`app-ref.json`: the manifest from `package.json`, every file with its sha256).
 */
import { spawnSync } from 'node:child_process'
import { createHash } from 'node:crypto'
import { existsSync, mkdtempSync, readdirSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join } from 'node:path'
import { fileURLToPath } from 'node:url'
import type { AppMeta } from './app.ts'

/** The darklua release the bundles are pinned to (the host's own build uses the same one). */
export const DARKLUA_VERSION = '0.18.0'
const DARKLUA_CONFIG = fileURLToPath(new URL('../darklua.json', import.meta.url))

function darklua(): string {
  const candidates = ['darklua', join(homedir(), '.cargo', 'bin', process.platform === 'win32' ? 'darklua.exe' : 'darklua')]
  for (const candidate of candidates) {
    const run = spawnSync(candidate, ['--version'], { encoding: 'utf8' })
    if (run.status !== 0) continue
    const version = run.stdout.trim()
    if (version !== `darklua ${DARKLUA_VERSION}`)
      throw new Error(`${candidate} is "${version}", the build is pinned to darklua ${DARKLUA_VERSION}:\n  cargo install darklua --version ${DARKLUA_VERSION} --locked --force`)
    return candidate
  }
  throw new Error(`darklua not found. It bundles the Luau backend; install the pinned version:\n  cargo install darklua --version ${DARKLUA_VERSION} --locked`)
}

/** The backend as the host runs it: `entry` with its relative requires inlined. */
export function bundleLuau(entry: string): string {
  const tool = darklua()
  const scratch = mkdtempSync(join(tmpdir(), 'vos-darklua-'))
  try {
    const output = join(scratch, 'out.luau')
    const run = spawnSync(tool, ['process', '--config', DARKLUA_CONFIG, entry, output], { cwd: dirname(entry), encoding: 'utf8' })
    if (run.status !== 0) throw new Error(`darklua could not bundle ${entry}:\n${`${run.stderr}${run.stdout}`.trim()}`)
    return readFileSync(output, 'utf8')
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

const sha256 = (bytes: string | Buffer): string => createHash('sha256').update(bytes).digest('hex')

/** Every file under `dir`, relative to it with `/`, in a stable order. */
export function filesUnder(dir: string, prefix = ''): string[] {
  if (!existsSync(join(dir, prefix))) return []
  return readdirSync(join(dir, prefix), { withFileTypes: true })
    .sort((a, b) => (a.name < b.name ? -1 : 1))
    .flatMap((entry) => {
      const path = prefix === '' ? entry.name : `${prefix}/${entry.name}`
      return entry.isDirectory() ? filesUnder(dir, path) : [path]
    })
}

/** Where the app ref carries the app's contract: the host's bus holds the app to it. */
const CONTRACT_FILE = 'openrpc.json'

/** The app ref of `outDir`, which already holds the built page in `staticDir` (and the contract, when there is one). */
export function appRef(app: AppMeta, outDir: string, bundle: string, staticDir: string, contract?: Buffer): object {
  const files = [{ path: 'app.luau', url: 'app.luau', sha256: sha256(bundle) }]
  if (contract) files.push({ path: CONTRACT_FILE, url: CONTRACT_FILE, sha256: sha256(contract) })
  for (const path of filesUnder(join(outDir, staticDir))) {
    const listed = `${staticDir}/${path}`
    files.push({ path: listed, url: listed, sha256: sha256(readFileSync(join(outDir, listed))) })
  }
  return {
    manifest: {
      id: app.id,
      name: app.title,
      version: app.version,
      main: 'app.luau',
      // what the page needs (MCP Apps' `_meta.ui`): the host turns it into the page's content security policy and the frame's `allow`
      ui: {
        root: staticDir,
        entry: 'index.html',
        ...(app.csp ? { csp: app.csp } : {}),
        ...(app.permissions ? { permissions: app.permissions } : {})
      },
      ...(contract ? { contract: CONTRACT_FILE } : {}),
      migrations: [],
      ...(app.icon ? { icon: app.icon } : {}),
      ...(app.tint ? { tint: app.tint } : {}),
      ...(app.description ? { description: app.description } : {}),
      ...(app.window ? { window: app.window } : {})
    },
    files
  }
}

/** Write `app.luau`, then `app-ref.json` last: a reader that follows the ref finds everything it lists. */
export function writeApp(app: AppMeta, outDir: string, bundle: string, staticDir: string, contract?: Buffer): void {
  writeFileSync(join(outDir, 'app.luau'), bundle)
  if (contract) writeFileSync(join(outDir, CONTRACT_FILE), contract)
  writeFileSync(join(outDir, 'app-ref.json'), `${JSON.stringify(appRef(app, outDir, bundle, staticDir, contract), null, 2)}\n`)
}
