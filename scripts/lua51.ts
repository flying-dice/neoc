/**
 * Bundling a Lua 5.1 app with darklua (config: .darklua.json), shared by `build-apps.ts` and `lua51-build.ts`.
 *
 * Apps are plain Lua 5.1 and are shipped as plain Lua 5.1: darklua only bundles an app's relative `require`s into
 * one chunk (and strips comments). `@agentos/*` requires stay as runtime `require` calls (the host provides those
 * modules). Nothing is lowered, so an app is exactly the Lua it is written in.
 *
 * The generator is `retain_lines` (with `remove_comments`), so line numbers in an error from the VM point at the
 * source: a file keeps every statement on its line (an app with no relative require is line for line its
 * `app.lua`). Bundling a module hoists it to the top of the file: the module's lines move down by a small
 * constant (+2 for `calc.lua`) and the entry's by the size of the modules before it. The `readable` generator
 * reformats every line and gives no such mapping. (`remove_types` is on only because darklua's own bundle
 * prelude carries a type annotation.)
 *
 * Unlike the lint tools, building cannot skip when darklua is missing: the output is what the host runs.
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join, resolve } from 'node:path'

export const root = resolve(import.meta.dir, '..')

/** The darklua release the generated output is pinned to (`cargo install darklua --version 0.18.0 --locked`). */
export const DARKLUA_VERSION = '0.18.0'

const CONFIG = join(root, '.darklua.json')

/** `darklua` on PATH or at ~/.cargo/bin/darklua. */
export function findDarklua(): string | undefined {
  const which = spawnSync('which', ['darklua'], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  const cargo = join(homedir(), '.cargo/bin/darklua')
  return existsSync(cargo) ? cargo : undefined
}

/** The pinned darklua, or a clear failure (message and exit code 1) when it is missing or another version. */
export function requireDarklua(): string {
  const darklua = findDarklua()
  if (!darklua) {
    console.error(
      `darklua not found. It bundles the Lua apps; install the pinned version:\n` +
        `  cargo install darklua --version ${DARKLUA_VERSION} --locked\n` +
        `(or put darklua ${DARKLUA_VERSION} on PATH, e.g. brew install darklua)`
    )
    process.exit(1)
  }
  const version = spawnSync(darklua, ['--version'], { encoding: 'utf8' }).stdout.trim()
  if (version !== `darklua ${DARKLUA_VERSION}`) {
    console.error(
      `${darklua} is "${version}", the build is pinned to darklua ${DARKLUA_VERSION} (its output is committed and checked):\n` +
        `  cargo install darklua --version ${DARKLUA_VERSION} --locked --force`
    )
    process.exit(1)
  }
  return darklua
}

/**
 * The Lua 5.1 text of the Lua file `entry`, with its relative requires bundled in.
 * Throws with darklua's message when the file does not process.
 */
export function toLua51(darklua: string, entry: string): string {
  const scratch = mkdtempSync(join(tmpdir(), 'agentos-darklua-'))
  try {
    const output = join(scratch, 'out.lua')
    const run = spawnSync(darklua, ['process', '--config', CONFIG, entry, output], { cwd: root, encoding: 'utf8' })
    if (run.status !== 0) throw new Error(`${run.stderr}${run.stdout}`.trim() || `darklua exited ${run.status}`)
    return readFileSync(output, 'utf8')
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}
