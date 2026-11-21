#!/usr/bin/env bun
/**
 * Checks every Luau app and fixture with `luau-lsp analyze` in strict mode against the generated declarations
 * (types/generated/vos.d.luau: the `app` API, `vos:ui`, the std:/lib:/vnd: modules an app is offered, and the one
 * `declare require`; written by vos-codegen).
 *
 *  1. Positive: every `.luau` file of an app or an app fixture (`apps/`, `crates/<crate>/tests/fixtures/`,
 *     `scripts/fixtures/` outside `negative/`) starts with `--!strict` and checks clean: no type error, no lint
 *     warning. `scripts/fixtures/usage.luau` also requires every module an app is offered (read from the
 *     declarations), so a module that stops loading is flagged there.
 *  2. Negative: every file in a `negative/` folder marks the lines that must be flagged with
 *     `-- expect-error: <text>`: luau-lsp must give exactly one finding per marker on that line (a line with two
 *     findings carries two markers), each marker matching a different finding's message up to its `caused by:`
 *     lines, and none on an unmarked line. `NEGATIVE_MARKERS` pins how many markers each file holds, so deleting a
 *     wrong line with its marker is an edit of that table. This is what stops the gate passing silently when the
 *     declarations stop loading (`app` and `ui` would be `any`, and nothing would be flagged).
 *  3. An app's own modules: the first pass sees `require("./calc")` as `any` (luau-lsp resolves a relative `require`
 *     to the module's type only while `require` is its built-in, and the declarations replace it with the one
 *     `declare require` that types `vos:*` and the std:/lib:/vnd: modules). So a second pass runs over the same
 *     files against the declarations with that `declare require` cut out: `require("./calc")` then has the module's
 *     real type and a wrong call into it is an error. In that pass `vos:*` and the offered std:/lib:/vnd: names are
 *     "Unknown require" (nothing resolves them) and those findings are dropped; every other finding is real, and so
 *     is an "Unknown require" of anything else: a refused `lib:test`, a typo such as `lib:jsno`, a missing module.
 *     A negative fixture's findings are those of both passes. (The editor still shows `any` for an app's own modules;
 *     this gate is what catches the errors.)
 *  4. Requires: every literal `require` of an app source is a relative path, `vos:app`, `vos:ui` or a module the
 *     declarations offer (the second pass's check, stated as a plain rule with a clearer message).
 *
 * luau-lsp is found on PATH or at ~/.local/bin/luau-lsp and must be the pinned version (its messages change between
 * releases). When it is missing the check SKIPS with a message and exits 0 on a developer machine, and FAILS in the
 * quality gate (VOS_GATE=1 or CI set, see scripts/gate.ts).
 *
 * Usage: bun scripts/lint-luau.ts
 */
import { Glob } from 'bun'
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { join, resolve } from 'node:path'
import { skipTool } from './gate'

const root = resolve(import.meta.dir, '..')

/** The luau-lsp release the declarations and the negative fixtures are pinned to (neoc's tests pin the same one). */
export const LUAU_LSP_VERSION = '1.70.1'

/** The definitions file every app is checked against. */
export const DEFINITIONS = 'types/generated/vos.d.luau'

/**
 * How many `-- expect-error` markers each negative fixture holds. Deleting a wrong line together with its marker
 * would otherwise shrink what is checked without a failure: it changes the count, which is edited here on purpose,
 * in the same change.
 */
export const NEGATIVE_MARKERS: Record<string, number> = {
  'scripts/fixtures/negative/wrong_api_use.luau': 15,
  'scripts/fixtures/negative/wrong_module_use.luau': 5,
  'scripts/fixtures/negative/wrong_requires.luau': 16,
  'scripts/fixtures/negative/wrong_ui_usage.luau': 14
}

/** Where apps and their fixtures live. */
const SOURCES = ['apps/**/*.luau', 'crates/*/tests/fixtures/**/*.luau', 'scripts/fixtures/**/*.luau']
const SKIP = /node_modules|\.claude|\/target\//

/** One finding of `luau-lsp analyze`: the 1-based line, the message up to its `caused by:` lines, the whole message. */
export interface Finding {
  line: number
  primary: string
  full: string
}

export function finding(line: number, message: string): Finding {
  const lines = message.split('\n')
  const cut = lines.findIndex((l) => l.startsWith('caused by:'))
  const head = (cut === -1 ? lines : lines.slice(0, cut)).map((l) => l.trim())
  return { line, primary: head.join(' '), full: message }
}

/** The markers on each marked line of `source`: `-- expect-error: <text>`, as many as the line has findings. */
export function expectedErrors(name: string, source: string): { line: number; markers: string[] }[] {
  const found: { line: number; markers: string[] }[] = []
  source.split('\n').forEach((text, index) => {
    if (!text.includes('-- expect-error')) return
    const markers = text
      .split('-- expect-error')
      .slice(1)
      .map((marker) => {
        const body = marker.startsWith(':') ? marker.slice(1).trim() : ''
        if (body === '') throw new Error(`${name}:${index + 1}: \`-- expect-error\` needs a \`: <text>\``)
        return body
      })
    found.push({ line: index + 1, markers })
  })
  return found
}

/** What is wrong between the markers of a negative fixture and what luau-lsp found in it. */
export function checkNegative(name: string, source: string, findings: Finding[]): string[] {
  const problems: string[] = []
  const expected = expectedErrors(name, source)
  if (expected.length === 0) problems.push(`${name} has no \`-- expect-error\` marker`)
  for (const { line, markers } of expected) {
    const onLine = findings.filter((f) => f.line === line)
    if (onLine.length !== markers.length) {
      problems.push(
        `${name}:${line}: ${markers.length} marker(s), luau-lsp gave ${onLine.length} finding(s): ${JSON.stringify(onLine.map((f) => f.primary))}`
      )
      continue
    }
    for (const marker of markers) {
      const at = onLine.findIndex((f) => f.primary.includes(marker))
      if (at === -1) {
        problems.push(`${name}:${line}: expected \`${marker}\`, got ${JSON.stringify(onLine.map((f) => f.primary))}`)
      } else {
        onLine.splice(at, 1)
      }
    }
  }
  for (const f of findings.filter((f) => !expected.some((e) => e.line === f.line))) {
    problems.push(`${name}:${f.line}: ${f.full} (a line not marked expect-error)`)
  }
  return problems
}

/** The number of markers in `source`. */
export function markerCount(name: string, source: string): number {
  return expectedErrors(name, source).reduce((sum, e) => sum + e.markers.length, 0)
}

/**
 * luau-lsp's findings by file from `analyze --formatter=plain` output.
 *
 * The plain format is `path:line:col-col: (W0) Kind: message`, a message of several lines continuing on the lines
 * after it. Any other output (`[ERROR]` when the definitions file does not load, a syntax error in it) is an error:
 * a broken declarations file must not look like a clean one.
 */
export function parseAnalysis(output: string, files: string[]): Map<string, Finding[]> {
  const raw = new Map<string, { line: number; message: string }[]>()
  let last: { line: number; message: string } | undefined
  for (const line of output.split('\n')) {
    if (line === '' || line.startsWith('[INFO]')) continue
    if (line.startsWith('[ERROR]') || line.includes('vos.d.luau')) {
      throw new Error(`luau-lsp could not use ${DEFINITIONS}: ${line}`)
    }
    const hit = files.find((file) => line.startsWith(`${file}:`))
    const detail = hit === undefined ? null : /^(\d+):\d+(?:-\d+)?: \(\w+\) (.*)$/.exec(line.slice(hit.length + 1))
    if (hit !== undefined && detail !== null) {
      last = { line: Number(detail[1]), message: detail[2] ?? '' }
      raw.set(hit, [...(raw.get(hit) ?? []), last])
    } else if (last !== undefined) {
      last.message += `\n${line}` // the rest of the message above
    } else {
      throw new Error(`unexpected luau-lsp output: ${line}`)
    }
  }
  return new Map([...raw].map(([file, found]) => [file, found.map((f) => finding(f.line, f.message))]))
}

/** The `require("...")` names a Luau source uses, with their lines (comments are skipped). */
export function requiredNames(source: string): { name: string; line: number }[] {
  const found: { name: string; line: number }[] = []
  source.split('\n').forEach((text, index) => {
    const code = text.replace(/--.*$/, '')
    for (const m of code.matchAll(/\brequire\s*\(?\s*(['"])(.*?)\1/g)) found.push({ name: m[2] ?? '', line: index + 1 })
  })
  return found
}

/** The module names the declarations' `declare require` has an overload for, with their types, in order. */
export function overloads(definitions: string): { name: string; type: string }[] {
  const start = definitions.indexOf('declare require:')
  const found: { name: string; type: string }[] = []
  for (const line of definitions.slice(start).split('\n').slice(1)) {
    const m = /^\s*(?:&\s*)?\(\(path: "([^"]+)"\) -> (\S+)\)\s*$/.exec(line)
    if (m) found.push({ name: m[1] ?? '', type: m[2] ?? '' })
  }
  return found
}

/** The modules an app may require: the declarations' overloads whose type is not an unusable one. */
export function offeredModules(definitions: string): Set<string> {
  return new Set(overloads(definitions).filter((o) => !o.type.startsWith('vos_NotForApps_')).map((o) => o.name))
}

/** The declarations without their `declare require`: the last section of the file (`-- ===== require =====`). */
export function withoutRequire(definitions: string): string {
  const at = definitions.indexOf('\n-- ===== require =====')
  if (at === -1) throw new Error(`${DEFINITIONS} has no \`-- ===== require =====\` section to cut out`)
  return `${definitions.slice(0, at)}\n`
}

/**
 * The findings of the second pass (no `declare require`), less the "Unknown require" of a name the first pass
 * types: `vos:*` and the offered modules resolve to no file there, which says nothing about the app.
 */
export function dropKnownUnknownRequires(findings: Finding[], offered: Set<string>): Finding[] {
  return findings.filter((f) => {
    const m = /^TypeError: Unknown require: (.*)$/.exec(f.primary)
    if (!m) return true
    const name = (m[1] ?? '').split('/').pop()?.replace(/\.luau?$/, '') ?? ''
    return !offered.has(name)
  })
}

/** Findings of two passes over the same file, once each (the same line and message is one finding). */
export function mergeFindings(...passes: Finding[][]): Finding[] {
  const seen = new Set<string>()
  const merged: Finding[] = []
  for (const f of passes.flat()) {
    const key = `${f.line}|${f.primary}`
    if (!seen.has(key)) {
      seen.add(key)
      merged.push(f)
    }
  }
  return merged.sort((a, b) => a.line - b.line)
}

/** Is `name` a `require` an app may write: a relative path, `vos:app`/`vos:ui`, or an offered module? */
export function allowedRequire(name: string, offered: Set<string>): boolean {
  return name.startsWith('./') || name.startsWith('../') || offered.has(name)
}

function listSources(): { positive: string[]; negative: string[] } {
  const all = new Set<string>()
  for (const pattern of SOURCES) {
    for (const file of new Glob(pattern).scanSync({ cwd: root })) if (!SKIP.test(`/${file}`)) all.add(file)
  }
  const sorted = [...all].sort()
  return {
    negative: sorted.filter((f) => f.includes('/negative/')),
    positive: sorted.filter((f) => !f.includes('/negative/'))
  }
}

function findLuauLsp(): string | undefined {
  const which = spawnSync('which', ['luau-lsp'], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  const local = join(homedir(), '.local/bin/luau-lsp')
  return existsSync(local) ? local : undefined
}

function main(): void {
  const INSTALL =
    `install luau-lsp ${LUAU_LSP_VERSION}: the release binary from\n` +
    `  https://github.com/JohnnyMorganz/luau-lsp/releases/tag/${LUAU_LSP_VERSION} unpacked somewhere on PATH (or ~/.local/bin)`
  const luauLsp = findLuauLsp()
  if (!luauLsp) skipTool('lint:luau', `luau-lsp not found (${INSTALL})`)
  const version = spawnSync(luauLsp, ['--version'], { encoding: 'utf8' }).stdout.trim()
  if (version !== LUAU_LSP_VERSION) {
    console.error(`lint:luau FAILED: ${luauLsp} is "${version}", the fixtures are pinned to ${LUAU_LSP_VERSION}\n${INSTALL}`)
    process.exit(1)
  }

  const definitions = readFileSync(join(root, DEFINITIONS), 'utf8')
  const offered = offeredModules(definitions)
  const { positive, negative } = listSources()
  const problems: string[] = []

  // the pinned counts and the files agree
  const listed = Object.keys(NEGATIVE_MARKERS).sort()
  if (JSON.stringify(listed) !== JSON.stringify(negative)) {
    problems.push(`negative fixtures ${JSON.stringify(negative)} are not the ones NEGATIVE_MARKERS pins ${JSON.stringify(listed)}`)
  }
  for (const [file, expected] of Object.entries(NEGATIVE_MARKERS)) {
    const source = existsSync(join(root, file)) ? readFileSync(join(root, file), 'utf8') : ''
    const markers = markerCount(file, source)
    if (markers !== expected) problems.push(`${file} holds ${markers} markers, NEGATIVE_MARKERS says ${expected}`)
  }

  // the usage fixture requires every module an app is offered, `vos:app` and `vos:ui`
  const usage = 'scripts/fixtures/usage.luau'
  const usageSource = existsSync(join(root, usage)) ? readFileSync(join(root, usage), 'utf8') : ''
  const usageRequires = new Set(requiredNames(usageSource).map((r) => r.name))
  for (const name of offered) if (!usageRequires.has(name)) problems.push(`${usage} never requires ${name}`)

  // luau-lsp over everything, twice: against the declarations, then against the declarations without their `declare
  // require` (see the header: an app's own modules get their real types in the second pass)
  const files = [...positive, ...negative]
  const scratch = mkdtempSync(join(tmpdir(), 'vos-lint-luau-'))
  const analyze = (definitions: string): Map<string, Finding[]> => {
    const run = spawnSync(
      luauLsp,
      ['analyze', '--formatter=plain', '--platform=standard', `--definitions:@vos=${definitions}`, ...files],
      { cwd: root, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }
    )
    return parseAnalysis(`${run.stderr}\n${run.stdout}`, files)
  }
  const found = new Map<string, Finding[]>()
  try {
    const first = analyze(DEFINITIONS)
    // the file keeps its name: parseAnalysis treats any output that names it as a definitions failure
    const bare = join(scratch, 'vos.d.luau')
    writeFileSync(bare, withoutRequire(definitions))
    const second = analyze(bare)
    for (const file of files) {
      found.set(file, mergeFindings(first.get(file) ?? [], dropKnownUnknownRequires(second.get(file) ?? [], offered)))
    }
  } catch (error) {
    console.error(`lint:luau FAILED: ${(error as Error).message}`)
    process.exit(1)
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }

  for (const file of positive) {
    const source = readFileSync(join(root, file), 'utf8')
    if (!source.startsWith('--!strict\n')) problems.push(`${file} must start with \`--!strict\``)
    for (const f of found.get(file) ?? []) problems.push(`${file}:${f.line}: ${f.full}`)
    for (const { name, line } of requiredNames(source)) {
      if (!allowedRequire(name, offered)) {
        problems.push(
          `${file}:${line}: require("${name}") is not a module an app may require (a relative path, vos:app, vos:ui or an offered std:/lib:/vnd: module)`
        )
      }
    }
  }
  for (const file of negative) {
    const source = readFileSync(join(root, file), 'utf8')
    if (!source.startsWith('--!strict\n')) problems.push(`${file} must start with \`--!strict\``)
    problems.push(...checkNegative(file, source, found.get(file) ?? []))
  }

  if (problems.length > 0) {
    console.error(`lint:luau FAILED (luau-lsp ${version})\n${problems.join('\n')}`)
    process.exit(1)
  }
  const markers = Object.values(NEGATIVE_MARKERS).reduce((a, b) => a + b, 0)
  console.log(
    `lint:luau OK (${positive.length} sources clean in strict mode in two passes (with and without the typed require), ${negative.length} negative fixtures flag exactly their ${markers} markers; ` +
      `${offered.size} offered modules; luau-lsp ${version})`
  )
}

if (import.meta.main) main()
