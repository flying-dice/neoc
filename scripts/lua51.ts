/**
 * Luau -> Lua 5.1 with darklua (config: .darklua.json), shared by `build-apps.ts` and `codegen-lua.ts`.
 *
 * Apps and `@agentos/ui` are authored in typed Luau and shipped as plain Lua 5.1: darklua strips the types,
 * rewrites the Luau-only syntax and bundles an app's relative `require`s into one chunk. `@agentos/*` requires
 * stay as runtime `require` calls (the host provides those modules). The Luau standard-library members Lua 5.1
 * lacks come from `lua/prelude.luau`.
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
      `darklua not found. It turns the Luau apps into Lua 5.1; install the pinned version:\n` +
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
 * darklua writes a non-ASCII character in a string as `\u{...}`, which Lua 5.1 reads as the letter `u` and
 * a brace. Rewrite each one to the decimal byte escapes of its UTF-8 encoding. The match consumes `\\` pairs
 * first so an escaped backslash before a `u{` is left alone.
 */
export function utf8Escapes(lua: string): string {
  return lua.replace(/\\(\\|u\{([0-9a-fA-F]+)\})/g, (whole, _escape: string, hex: string | undefined) => {
    if (hex === undefined) return whole
    const bytes = Buffer.from(String.fromCodePoint(Number.parseInt(hex, 16)), 'utf8')
    return [...bytes].map((byte) => `\\${String(byte).padStart(3, '0')}`).join('')
  })
}

/**
 * The Lua 5.1 text of the Luau file `entry`, with its relative requires bundled in.
 * Throws with darklua's message when the file does not process.
 */
export function toLua51(darklua: string, entry: string): string {
  const scratch = mkdtempSync(join(tmpdir(), 'agentos-darklua-'))
  try {
    const output = join(scratch, 'out.lua')
    const run = spawnSync(darklua, ['process', '--config', CONFIG, entry, output], { cwd: root, encoding: 'utf8' })
    if (run.status !== 0) throw new Error(`${run.stderr}${run.stdout}`.trim() || `darklua exited ${run.status}`)
    return utf8Escapes(readFileSync(output, 'utf8'))
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}
