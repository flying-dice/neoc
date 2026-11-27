/**
 * Bundling a Luau backend with darklua (config: .darklua.json), for `luau-build.ts` (the Rust tests build their fixture
 * backends with it) and the tests of the pipeline. An app's own build (`packages/vite-plugin`) bundles its backend with the
 * same darklua and the same config.
 *
 * Backends are authored in Luau (`--!strict`, annotations, the whole language) and run as Luau: darklua bundles an
 * app's relative `require`s into one chunk (darklua resolves `./calc` to `calc.luau`), strips the comments and the
 * type annotations (the host does not use them, and darklua's own bundle prelude carries one). `vos:*` requires and
 * the host's neoc modules (`std:*`, `lib:*`, `vnd:*`) stay as runtime `require` calls (the host provides those
 * modules; `.darklua.json` excludes them, as `[:]` because a bare `:` is not a valid darklua glob). Nothing is
 * lowered: no rule rewrites Luau syntax, so what runs is the Luau the app is written in, less its types.
 *
 * The generator is `retain_lines` (with `remove_comments`), so line numbers in an error from the VM point at the
 * source: a file keeps every statement on its line (an app with no relative require is line for line its
 * `app.luau`). Bundling a module hoists it to the top of the file: the module's lines move down by a small
 * constant (+4 for `calc.luau`) and the entry's by the size of the modules before it. The `readable` generator
 * reformats every line and gives no such mapping.
 *
 * Unlike the lint tools, building cannot skip when darklua is missing: the output is what the host runs.
 */
import { spawnSync } from 'node:child_process'
import { mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { findTool } from './find-tool.ts'

export const root = resolve(import.meta.dir, '..')

/** The darklua release the generated output is pinned to (`cargo install darklua --version 0.18.0 --locked`). */
export const DARKLUA_VERSION = '0.18.0'

const CONFIG = join(root, '.darklua.json')

/** `darklua` on PATH or at ~/.cargo/bin/darklua. */
export function findDarklua(): string | undefined {
  return findTool('darklua', join(homedir(), '.cargo/bin/darklua'))
}

/** The pinned darklua, or a clear failure (message and exit code 1) when it is missing or another version. */
export function requireDarklua(): string {
  const darklua = findDarklua()
  if (!darklua) {
    console.error(
      `darklua not found. It bundles the Luau apps; install the pinned version:\n` +
        `  cargo install darklua --version ${DARKLUA_VERSION} --locked\n` +
        `(or put darklua ${DARKLUA_VERSION} on PATH, e.g. brew install darklua)`
    )
    process.exit(1)
  }
  const version = spawnSync(darklua, ['--version'], { encoding: 'utf8' }).stdout.trim()
  if (version !== `darklua ${DARKLUA_VERSION}`) {
    console.error(
      `${darklua} is "${version}", the build is pinned to darklua ${DARKLUA_VERSION} (its output is what the host runs):\n` +
        `  cargo install darklua --version ${DARKLUA_VERSION} --locked --force`
    )
    process.exit(1)
  }
  return darklua
}

/**
 * The Luau text of the Luau file `entry`, with its relative requires bundled in.
 * Throws with darklua's message when the file does not process.
 */
export function toLuau(darklua: string, entry: string): string {
  const scratch = mkdtempSync(join(tmpdir(), 'vos-darklua-'))
  try {
    const output = join(scratch, 'out.lua')
    const run = spawnSync(darklua, ['process', '--config', CONFIG, entry, output], { cwd: root, encoding: 'utf8' })
    if (run.status !== 0) throw new Error(`${run.stderr}${run.stdout}`.trim() || `darklua exited ${run.status}`)
    return readFileSync(output, 'utf8')
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}
