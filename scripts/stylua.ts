#!/usr/bin/env bun
/**
 * Lua formatting with StyLua (config: stylua.toml, syntax Lua51, ignores: .styluaignore), the Lua counterpart of
 * `cargo fmt`.
 *
 *   bun scripts/stylua.ts check    exit 1 if any .lua file is not formatted (part of `bun run lint`)
 *   bun scripts/stylua.ts format   rewrite the files in place (part of `bun run format`)
 *
 * `stylua` is found on PATH or at ~/.cargo/bin/stylua. When it is missing the script SKIPS with a message
 * and exits 0, like the rest of the optional tooling.
 */
import { spawnSync } from 'node:child_process'
import { existsSync } from 'node:fs'
import { homedir } from 'node:os'
import { join, resolve } from 'node:path'

const root = resolve(import.meta.dir, '..')
const mode = process.argv[2]
if (mode !== 'check' && mode !== 'format') {
  console.error('usage: bun scripts/stylua.ts check|format')
  process.exit(2)
}

function findStylua(): string | undefined {
  const which = spawnSync('which', ['stylua'], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  const cargo = join(homedir(), '.cargo/bin/stylua')
  return existsSync(cargo) ? cargo : undefined
}

const stylua = findStylua()
if (!stylua) {
  console.log(`stylua ${mode} SKIPPED: stylua not found (cargo install stylua --features lua51, or put it on PATH)`)
  process.exit(0)
}

// one stylua.toml at the root (call_parentheses = "NoSingleTable" keeps the table-call idiom, `ui.VStack { ... }`)
const args = ['--search-parent-directories', ...(mode === 'check' ? ['--check'] : []), '.']
const run = spawnSync(stylua, args, { cwd: root, stdio: 'inherit' })
if (run.status !== 0) {
  if (mode === 'check') console.error('stylua: Lua files need formatting: run `bun run format`')
  process.exit(run.status ?? 1)
}
console.log(`stylua ${mode} OK`)
