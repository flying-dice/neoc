#!/usr/bin/env bun
/**
 * Checks every Lua app and fixture with lua-language-server (LuaLS) `--check` against the generated definitions
 * (config: .luarc.json, definitions: types/generated/luals, written by `bun run codegen`).
 *
 *  1. Positive: every `.lua` file the config does not ignore (apps, lua/prelude.lua, the example app fixture) must
 *     check clean.
 *  2. Negative: every file in a `negative/` folder marks the lines that must be flagged with `-- expect-error`;
 *     LuaLS must flag exactly those lines. This is what stops the gate passing silently when the definitions
 *     stop loading (every `app` and `ui` use would then be an unknown global or `any`, so the negative check also
 *     fails when the definitions do not load).
 *
 * `lua-language-server` is found on PATH or at ~/.local/bin/lua-language-server and must be the pinned version
 * (its diagnostics change between releases). When it is missing the check SKIPS with a message and exits 0 on a
 * developer machine, and FAILS in the quality gate (AGENTOS_GATE=1 or CI set, see scripts/gate.ts).
 *
 * Usage: bun scripts/lint-lua.ts
 */
import { Glob } from 'bun'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join, relative, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { skipTool } from './gate'

const root = resolve(import.meta.dir, '..')

/** The LuaLS release the config and the negative fixtures are pinned to. */
export const LUALS_VERSION = '3.19.2'

const INSTALL =
  `install lua-language-server ${LUALS_VERSION}: brew install lua-language-server, or the release binary from\n` +
  `  https://github.com/LuaLS/lua-language-server/releases/tag/${LUALS_VERSION} unpacked somewhere on PATH (or ~/.local/bin)`

function findLuaLs(): string | undefined {
  const which = spawnSync('which', ['lua-language-server'], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  const local = join(homedir(), '.local/bin/lua-language-server')
  return existsSync(local) ? local : undefined
}

const luals = findLuaLs()
if (!luals) skipTool('lint:lua', `lua-language-server not found (${INSTALL})`)
const version = spawnSync(luals, ['--version'], { encoding: 'utf8' }).stdout.trim()
if (version !== LUALS_VERSION) {
  console.error(`lint:lua FAILED: ${luals} is "${version}", the config is pinned to ${LUALS_VERSION}\n${INSTALL}`)
  process.exit(1)
}

interface Diagnostic {
  code: string
  message: string
  range: { start: { line: number; character: number } }
}

/** LuaLS's findings by repo-relative file, from `--check` over the whole workspace (the config picks the files). */
function check(): Map<string, Diagnostic[]> {
  const scratch = mkdtempSync(join(tmpdir(), 'agentos-luals-'))
  try {
    const run = spawnSync(
      luals as string,
      [
        '--check',
        root,
        '--configpath',
        join(root, '.luarc.json'),
        '--checklevel',
        'Hint',
        '--check_format',
        'json',
        '--logpath',
        join(scratch, 'log'),
        '--metapath',
        join(scratch, 'meta')
      ],
      { cwd: root, encoding: 'utf8' }
    )
    const report = join(scratch, 'log', 'check.json')
    if (!existsSync(report)) {
      // LuaLS writes the report only when it found something
      if (/Diagnosis complete, no problems found/.test(`${run.stdout}${run.stderr}`) || run.status === 0) return new Map()
      console.error(`lint:lua FAILED: lua-language-server did not produce a report\n${run.stdout}${run.stderr}`)
      process.exit(1)
    }
    const raw = JSON.parse(readFileSync(report, 'utf8')) as Record<string, Diagnostic[]>
    return new Map(Object.entries(raw).map(([uri, found]) => [relative(root, fileURLToPath(uri)), found]))
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
}

const found = check()
const describe = (file: string, d: Diagnostic) =>
  `${file}:${d.range.start.line + 1}:${d.range.start.character + 1} [${d.code}] ${d.message.split('\n')[0]}`

const negative: string[] = []
for (const file of new Glob('**/negative/*.lua').scanSync({ cwd: root })) {
  if (!/node_modules|\.claude|target/.test(file)) negative.push(file)
}
negative.sort()
const positive = [...found.keys()].filter((file) => !negative.includes(file)).sort()

const problems: string[] = []
for (const file of positive) for (const d of found.get(file) ?? []) problems.push(describe(file, d))

for (const file of negative) {
  const lines = readFileSync(join(root, file), 'utf8').split('\n')
  const expected = lines.flatMap((l, i) => (l.includes('-- expect-error') ? [i + 1] : []))
  const flagged = new Map<number, Diagnostic>()
  for (const d of found.get(file) ?? []) flagged.set(d.range.start.line + 1, d)
  for (const line of expected) {
    if (!flagged.has(line)) problems.push(`${file}:${line}: expected a diagnostic here, lua-language-server accepted it`)
  }
  for (const [line, d] of flagged) {
    if (!expected.includes(line)) problems.push(`${describe(file, d)} (a line not marked expect-error)`)
  }
}

if (problems.length > 0) {
  console.error(`lint:lua FAILED\n${problems.join('\n')}`)
  process.exit(1)
}
console.log(
  `lint:lua OK (apps and sources clean, ${negative.length} negative fixture${negative.length === 1 ? '' : 's'} flagged as expected; ` +
    `lua-language-server ${version})`
)
