/**
 * The Luau tooling of an app project, wired into its scripts by `vos`: luau-lsp checks the backend (and the generated
 * router) in strict mode against the host's declarations (`types/generated/vos.d.luau`: the `app` API and every module an
 * app may require), and StyLua checks the format. A missing tool skips with a message on a developer machine and fails
 * in the gate (`VOS_GATE=1` or `CI`), as the repository's own checks do: a skipped check is not a pass.
 */
import { spawnSync } from 'node:child_process'
import { existsSync, mkdtempSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { homedir, tmpdir } from 'node:os'
import { dirname, join, relative, resolve } from 'node:path'

/** The luau-lsp release the declarations are written for (its messages change between releases). */
export const LUAU_LSP_VERSION = '1.70.1'

/** A tool the check needs is not installed (`analyze` in `throw` mode says so with this, rather than ending the process). */
export class ToolMissing extends Error {}

export const isGateRun = (): boolean => {
  if (process.env.VOS_GATE === '1') return true
  const ci = process.env.CI
  return ci !== undefined && ci !== '' && ci !== 'false' && ci !== '0'
}

/** A missing tool: a skip (exit 0) on a developer machine, a failure in the gate. */
function missing(check: string, message: string): never {
  if (isGateRun()) {
    console.error(`${check} FAILED: ${message}\n  (VOS_GATE or CI is set: a skipped check is not a pass)`)
    process.exit(1)
  }
  console.log(`${check} SKIPPED: ${message}`)
  process.exit(0)
}

function onPath(name: string, ...extra: string[]): string | undefined {
  const which = spawnSync('which', [name], { encoding: 'utf8' })
  if (which.status === 0 && which.stdout.trim()) return which.stdout.trim()
  return extra.find((candidate) => existsSync(candidate))
}

/** The Luau files of a project: every `.luau` outside `node_modules`, `dist` and hidden folders. */
export function luauFiles(project: string): string[] {
  const found: string[] = []
  const walk = (dir: string): void => {
    for (const name of readdirSync(dir)) {
      if ((name.startsWith('.') && name !== '.app') || name === 'node_modules' || name === 'dist') continue
      const path = join(dir, name)
      if (statSync(path).isDirectory()) walk(path)
      else if (name.endsWith('.luau')) found.push(relative(project, path))
    }
  }
  walk(project)
  return found.sort()
}

/** The host's declarations: `VOS_DEFINITIONS`, else `types/generated/vos.d.luau` of the repository the project is in. */
export function findDefinitions(project: string): string {
  const fromEnv = process.env.VOS_DEFINITIONS
  if (fromEnv) return resolve(fromEnv)
  for (let dir = resolve(project); ; dir = dirname(dir)) {
    const candidate = join(dir, 'types/generated/vos.d.luau')
    if (existsSync(candidate)) return candidate
    if (dirname(dir) === dir) break
  }
  throw new Error('types/generated/vos.d.luau not found above the project: set VOS_DEFINITIONS to the host\'s declarations')
}

/** The findings of luau-lsp's plain output: `path:line:col: (W0) Kind: message`, a message continuing on the lines after. */
export function parseFindings(output: string): string[] {
  const findings: string[] = []
  for (const line of output.split('\n')) {
    if (line === '' || line.startsWith('[INFO]')) continue
    if (/^\S.*:\d+:\d+(-\d+)?: \(\w+\) /.test(line)) findings.push(line)
    else if (findings.length > 0) findings[findings.length - 1] += `\n${line}`
    else findings.push(line)
  }
  return findings
}

/** The declarations without their `declare require`: the last section (`-- ===== require =====`). */
function withoutRequire(definitions: string): string {
  const at = definitions.indexOf('\n-- ===== require =====')
  if (at === -1) throw new Error('the declarations have no `-- ===== require =====` section')
  return `${definitions.slice(0, at)}\n`
}

/** The names the declarations' `declare require` has an overload for. */
function offeredModules(definitions: string): Set<string> {
  const start = definitions.indexOf('declare require:')
  const names = new Set<string>()
  for (const line of definitions.slice(start).split('\n').slice(1)) {
    const m = /^\s*(?:&\s*)?\(\(path: "([^"]+)"\)/.exec(line)
    if (m) names.add(m[1] as string)
  }
  return names
}

/**
 * Check every Luau file of the project in strict mode. Returns the findings (empty: clean). A missing tool ends the process
 * (a skip on a developer machine, a failure in the gate) unless `mode` is `throw`, where it is a [`ToolMissing`] (or an
 * `Error`, for a version the declarations are not written for) for the caller to decide, as the build does.
 */
export function analyze(project: string, mode: 'exit' | 'throw' = 'exit'): { files: string[]; findings: string[] } {
  const luauLsp = onPath('luau-lsp', join(homedir(), '.local/bin/luau-lsp'))
  if (!luauLsp) {
    const message = `luau-lsp not found: install the ${LUAU_LSP_VERSION} release binary from https://github.com/JohnnyMorganz/luau-lsp/releases/tag/${LUAU_LSP_VERSION} (on PATH or in ~/.local/bin)`
    if (mode === 'throw') throw new ToolMissing(message)
    missing('luau check', message)
  }
  const version = spawnSync(luauLsp, ['--version'], { encoding: 'utf8' }).stdout.trim()
  if (version !== LUAU_LSP_VERSION) {
    const message = `${luauLsp} is "${version}", the declarations are written for ${LUAU_LSP_VERSION}`
    if (mode === 'throw') throw new Error(`luau check: ${message}`)
    console.error(`luau check FAILED: ${message}`)
    process.exit(1)
  }
  const definitionsPath = findDefinitions(project)
  const definitions = readFileSync(definitionsPath, 'utf8')
  const offered = offeredModules(definitions)
  const files = luauFiles(project)
  const problems: string[] = []
  for (const file of files) {
    if (!readFileSync(join(project, file), 'utf8').startsWith('--!strict\n')) problems.push(`${file}: must start with \`--!strict\``)
  }

  // two passes, as the repository's `lint:luau`: against the declarations, then without their `declare require`, where
  // an app's own modules (`require("../.app/rpc")`) get their real types; there `vos:*` and the offered modules
  // resolve to no file, which says nothing about the app
  const scratch = mkdtempSync(join(tmpdir(), 'vos-luau-'))
  try {
    const run = (definitionsFile: string): string[] => {
      const result = spawnSync(
        luauLsp,
        ['analyze', '--formatter=plain', '--platform=standard', `--definitions:@vos=${definitionsFile}`, ...files],
        { cwd: project, encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }
      )
      return parseFindings(`${result.stderr}\n${result.stdout}`)
    }
    const first = run(definitionsPath)
    const bare = join(scratch, 'vos.d.luau')
    writeFileSync(bare, withoutRequire(definitions))
    const second = run(bare).filter((finding) => {
      const m = /TypeError: Unknown require: (.*)$/m.exec(finding)
      return !m || !offered.has(((m[1] ?? '').split('/').pop() ?? '').replace(/\.luau?$/, ''))
    })
    const seen = new Set<string>()
    for (const finding of [...first, ...second]) {
      if (!seen.has(finding)) {
        seen.add(finding)
        problems.push(finding)
      }
    }
  } finally {
    rmSync(scratch, { recursive: true, force: true })
  }
  return { files, findings: problems }
}

/** StyLua over the project's own Luau (not the generated files in `.app/`). `write` formats, else it checks. */
export function stylua(project: string, write: boolean): boolean {
  const tool = onPath('stylua', join(homedir(), '.cargo/bin/stylua'))
  if (!tool) missing('stylua', 'stylua not found (cargo install stylua --features luau)')
  const files = luauFiles(project).filter((f) => !f.startsWith('.app/'))
  if (files.length === 0) return true
  const run = spawnSync(tool, ['--search-parent-directories', ...(write ? [] : ['--check']), ...files], { cwd: project, encoding: 'utf8' })
  if (run.status !== 0) console.error(`${run.stdout}${run.stderr}`.trim())
  return run.status === 0
}
