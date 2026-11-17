#!/usr/bin/env bun
/**
 * Enforces the AgentOS process and package boundaries (AGENTS.md → "Boundaries",
 * apps/README.md → "The SDK is the only contract").
 *
 * Rules
 *  1. An app module (apps/<id>, not desktop) imports only @agentos/sdk (root, app/main,
 *     app/renderer), @agentos/ui, svelte, zod and its own files. Never electron, Node builtins,
 *     the host, another app or @agentos/sdk/host/*. Test support may also use TEST_KIT.
 *  2. Inside an app, src/renderer and src/main never import each other (they talk through
 *     api.invoke / ctx.handle / ctx.emit); src/main is headless; src/shared is pure.
 *     Apps never touch `window.agentos`; they use useAppApi().
 *  3. No relative import leaves its workspace package (test support may reach the repo's e2e/).
 *  4. electron is imported only by the desktop main and preload processes and, type-only, by
 *     packages/sdk/src/host. ipcMain / ipcRenderer / contextBridge / webContents.send appear only
 *     in IPC_ROOTS: every other message goes through the SDK protocol.
 *  5. Code that runs sandboxed (desktop renderer and preload, packages/ui, packages/sdk) imports no
 *     Node builtins, and each desktop process imports only its own SDK entry points.
 *  6. @agentos/sdk/testing only in test support, Storybook and web mode; @agentos/app-* only in
 *     the desktop's installedApps registries; packages/ui imports no other @agentos package.
 *  7. No {@html}, innerHTML, outerHTML, insertAdjacentHTML or document.write: agent and tool
 *     output is untrusted. HTML_ALLOWED lists the reviewed exceptions.
 *  8. BrowserWindow webPreferences never weaken the sandbox.
 *
 * Usage: bun scripts/check-boundaries.ts
 */
import { Glob } from 'bun'
import { builtinModules } from 'node:module'
import { dirname, join, relative, resolve, sep } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const SOURCE_GLOBS = ['apps/*/{src,e2e,.storybook}/**/*.{ts,svelte,js,mjs}', 'packages/*/**/*.{ts,svelte,js,mjs}']
const SKIP = /(^|\/)(node_modules|storybook-static|dist|out)\/|^packages\/[^/]+\/[^/]+\.config\.\w+$/

/** What an app's shipped code may import. A new entry is an architecture decision: say why in the MR. */
const APP_IMPORTS = [
  '@agentos/sdk',
  '@agentos/sdk/app/main',
  '@agentos/sdk/app/renderer',
  '@agentos/ui',
  '@agentos/ui/*',
  'svelte',
  'svelte/*',
  'zod'
]
/** Extra imports for test support (unit tests, stories, Storybook config, journeys, mocks). */
const TEST_KIT = [
  '@agentos/sdk/testing',
  '@agentos/storybook',
  '@agentos/storybook/*',
  '@storybook/*',
  'storybook',
  'storybook/*',
  'bun:test',
  '@playwright/test',
  '@axe-core/playwright'
]

const ELECTRON_ALLOWED = ['apps/desktop/src/main/', 'apps/desktop/src/preload/', 'packages/sdk/src/host/main.ts', 'packages/sdk/src/host/preload.ts']
const IPC_ROOTS = ['apps/desktop/src/main/index.ts', 'apps/desktop/src/preload/index.ts', 'packages/sdk/src/host/']
const APP_REGISTRIES = ['apps/desktop/src/main/platform/installedApps.ts', 'apps/desktop/src/renderer/src/lib/installedApps.ts']
/** Static, reviewed markup only. Never pass agent, tool or user text to these. */
const HTML_ALLOWED = ['packages/ui/src/atoms/Icon.svelte'] // SVG strings from the constant icons map
const SANDBOXED = ['apps/desktop/src/renderer/', 'apps/desktop/src/preload/', 'packages/ui/src/', 'packages/sdk/src/']
const NODE_ALLOWED_IN_SANDBOXED = ['packages/sdk/src/host/main.ts'] // runs in the main process
/** SDK entry points each desktop process must not import (they belong to another process). */
const FOREIGN_SDK_ENTRIES: Record<string, string[]> = {
  'apps/desktop/src/main/': ['@agentos/sdk/host/renderer', '@agentos/sdk/host/preload', '@agentos/sdk/app/renderer'],
  'apps/desktop/src/preload/': ['@agentos/sdk/host/main', '@agentos/sdk/host/renderer', '@agentos/sdk/app/main', '@agentos/sdk/app/renderer'],
  'apps/desktop/src/renderer/': ['@agentos/sdk/host/main', '@agentos/sdk/host/preload', '@agentos/sdk/app/main']
}

const WEAK_WEB_PREFERENCES: [RegExp, string][] = [
  [/\bsandbox\s*:\s*false\b/, 'sandbox: false'],
  [/\bcontextIsolation\s*:\s*false\b/, 'contextIsolation: false'],
  [/\bnodeIntegration\w*\s*:\s*true\b/, 'nodeIntegration*: true'],
  [/\bwebviewTag\s*:\s*true\b/, 'webviewTag: true'],
  [/\bwebSecurity\s*:\s*false\b/, 'webSecurity: false'],
  [/\ballowRunningInsecureContent\s*:\s*true\b/, 'allowRunningInsecureContent: true']
]

const under = (rel: string, prefixes: string[]): boolean => prefixes.some((p) => (p.endsWith('/') ? rel.startsWith(p) : rel === p))
const matches = (spec: string, patterns: string[]): boolean =>
  patterns.some((p) => (p.endsWith('/*') ? spec === p.slice(0, -2) || spec.startsWith(p.slice(0, -1)) : spec === p))
const isBuiltin = (spec: string): boolean => spec.startsWith('node:') || builtinModules.includes(spec.split('/')[0])
const isElectron = (spec: string): boolean => spec === 'electron' || spec.startsWith('electron/')

function isTestSupport(rel: string): boolean {
  return (
    /\.(test|spec)\.ts$/.test(rel) ||
    /\.stories\.(svelte|ts)$/.test(rel) ||
    /^(apps|packages)\/[^/]+\/(e2e|\.storybook)\//.test(rel) ||
    /^apps\/[^/]+\/src\/(testing\.ts$|mock\/)/.test(rel) ||
    rel.startsWith('packages/storybook/') ||
    rel.startsWith('packages/sdk/src/testing/')
  )
}
const isWebMode = (rel: string): boolean => rel.startsWith('apps/desktop/src/renderer/src/web/')

/** Which side of an app module a path is on. */
function appPart(rel: string): 'main' | 'renderer' | 'shared' | null {
  const part = rel.match(/^apps\/[^/]+\/src\/(main|renderer|shared)\//)?.[1]
  return part === 'main' || part === 'renderer' || part === 'shared' ? part : null
}

// ── Source preparation ───────────────────────────────────────────────────

const blank = (s: string): string => s.replace(/[^\n]/g, ' ')

/** Blanks // and block comments, keeping strings, regex literals and line numbers intact. */
function stripComments(code: string): string {
  let out = ''
  let quote: string | null = null
  let lastSignificant = ''
  for (let i = 0; i < code.length; ) {
    const c = code[i]
    const next = code[i + 1]
    if (quote) {
      if (c === '\\') {
        out += code.slice(i, i + 2)
        i += 2
        continue
      }
      if (c === quote || (c === '\n' && quote !== '`')) quote = null
      out += c
      i++
      continue
    }
    if (c === '/' && next === '/') {
      const end = code.indexOf('\n', i)
      const stop = end < 0 ? code.length : end
      out += blank(code.slice(i, stop))
      i = stop
      continue
    }
    if (c === '/' && next === '*') {
      const end = code.indexOf('*/', i + 2)
      const stop = end < 0 ? code.length : end + 2
      out += blank(code.slice(i, stop))
      i = stop
      continue
    }
    if (c === '/' && (lastSignificant === '' || '(,=:[!&|?{};'.includes(lastSignificant))) {
      // Regex literal: copy through its closing slash so `//` or quotes inside it are not misread.
      let j = i + 1
      let inClass = false
      for (; j < code.length && code[j] !== '\n'; j++) {
        if (code[j] === '\\') j++
        else if (code[j] === '[') inClass = true
        else if (code[j] === ']') inClass = false
        else if (code[j] === '/' && !inClass) break
      }
      out += code.slice(i, j + 1)
      i = j + 1
      lastSignificant = '/'
      continue
    }
    if (c === '"' || c === "'" || c === '`') quote = c
    if (!/\s/.test(c)) lastSignificant = c
    out += c
    i++
  }
  return out
}

/** For a .svelte file: the script code (comments stripped) and the markup, both line-aligned with the source. */
function split(rel: string, src: string): { code: string; markup: string } {
  if (!rel.endsWith('.svelte')) return { code: stripComments(src), markup: '' }
  const scripts = /<script\b[^>]*>([\s\S]*?)<\/script>/g
  let code = ''
  let last = 0
  for (const m of src.matchAll(scripts)) {
    const start = (m.index ?? 0) + m[0].indexOf('>') + 1
    code += blank(src.slice(last, start)) + m[1]
    last = start + m[1].length
  }
  code += blank(src.slice(last))
  const markup = src
    .replace(/<script\b[\s\S]*?<\/script>/g, blank)
    .replace(/<style\b[\s\S]*?<\/style>/g, blank)
    .replace(/<!--[\s\S]*?-->/g, blank)
  return { code: stripComments(code), markup }
}

const lineAt = (text: string, index: number): number => text.slice(0, index).split('\n').length

interface Import {
  spec: string
  clause: string
  typeOnly: boolean
  line: number
}

const STATIC_IMPORT = /\b(?:import|export)\s+(type\s+)?(?:([\w$*{}\s,]+?)\s+from\s+)?['"]([^'"\n]+)['"]/g
const DYNAMIC_IMPORT = /\b(?:import|require)\s*\(\s*['"]([^'"\n]+)['"]\s*\)/g

function importsOf(code: string): Import[] {
  const found: Import[] = []
  for (const m of code.matchAll(STATIC_IMPORT))
    found.push({ spec: m[3], clause: m[2] ?? '', typeOnly: !!m[1], line: lineAt(code, m.index ?? 0) })
  for (const m of code.matchAll(DYNAMIC_IMPORT))
    found.push({ spec: m[1], clause: '', typeOnly: false, line: lineAt(code, m.index ?? 0) })
  return found
}

// ── Rules ────────────────────────────────────────────────────────────────

const errors: string[] = []
const report = (rel: string, line: number, message: string): void => void errors.push(`${rel}:${line}: ${message}`)

function appImportProblem(spec: string): string {
  if (isElectron(spec)) return 'apps never import electron: reach the platform through ctx (app/main) or useAppApi() (app/renderer)'
  if (spec.startsWith('@agentos/sdk/host/')) return `'${spec}' is host-only: apps use @agentos/sdk/app/main or @agentos/sdk/app/renderer`
  if (spec === '@agentos/desktop' || spec.startsWith('@agentos/app-'))
    return `apps never import the host or another app ('${spec}'): call other apps' tools with api.tools.call`
  if (isBuiltin(spec)) return `apps get no Node builtins ('${spec}'): app code must be browser-safe; extend AppMainContext if a capability is missing`
  return `'${spec}' is not on the app allow-list (sdk, ui, svelte, zod): a new app dependency is an architecture decision; justify it in the MR and add it to APP_IMPORTS`
}

function checkAppImport(rel: string, imp: Import, abs: string, test: boolean): void {
  if (imp.spec.startsWith('.')) {
    if (test) return
    const from = appPart(rel)
    const to = appPart(relative(ROOT, resolve(dirname(abs), imp.spec)).split(sep).join('/') + '/')
    if (from && to && from !== to && (from === 'shared' || to !== 'shared'))
      report(rel, imp.line, from === 'shared'
        ? `src/shared is pure logic shared by both sides; it may not import src/${to} ('${imp.spec}')`
        : `src/${from} may not import src/${to} ('${imp.spec}'): renderer and main run in different processes; use api.invoke / ctx.handle / ctx.emit and put shared code in src/shared`)
    return
  }
  if (!matches(imp.spec, APP_IMPORTS) && !(test && (matches(imp.spec, TEST_KIT) || isBuiltin(imp.spec)))) {
    report(rel, imp.line, appImportProblem(imp.spec))
    return
  }
  if (test) return
  const part = appPart(rel)
  if (part === 'main' && matches(imp.spec, ['svelte', 'svelte/*', '@agentos/ui', '@agentos/ui/*', '@agentos/sdk/app/renderer']))
    report(rel, imp.line, `src/main is headless: '${imp.spec}' belongs to the renderer`)
  if (part === 'renderer' && imp.spec === '@agentos/sdk/app/main')
    report(rel, imp.line, "src/renderer may not import '@agentos/sdk/app/main': the main context lives in another process")
  if (part === 'shared' && matches(imp.spec, ['svelte', 'svelte/*', '@agentos/ui', '@agentos/ui/*', '@agentos/sdk/app/*']))
    report(rel, imp.line, `src/shared is pure (no framework or SDK runtime) so bun tests run it directly; '${imp.spec}' is not allowed`)
}

function checkImport(rel: string, imp: Import, abs: string): void {
  const { spec } = imp
  const test = isTestSupport(rel)
  const app = rel.match(/^apps\/([^/]+)\//)?.[1]

  if (spec.startsWith('.')) {
    const pkg = rel.split('/').slice(0, 2).join('/')
    const target = relative(ROOT, resolve(dirname(abs), spec)).split(sep).join('/')
    const escapes = target !== pkg && !target.startsWith(pkg + '/')
    if (escapes && !(test && target.startsWith('e2e/')))
      report(rel, imp.line, `relative import '${spec}' leaves ${pkg}: cross packages only through their published entry points`)
  }

  if (app && app !== 'desktop') return checkAppImport(rel, imp, abs, test)

  if (isElectron(spec)) {
    if (!under(rel, ELECTRON_ALLOWED))
      report(rel, imp.line, "only the desktop main and preload processes import 'electron'")
    else if (rel.startsWith('packages/sdk/') && !imp.typeOnly)
      report(rel, imp.line, 'the SDK takes Electron by injection: use `import type` from electron')
    const raw = imp.clause.match(/\b(ipcMain|ipcRenderer|contextBridge)\b/)
    if (raw && !under(rel, IPC_ROOTS))
      report(rel, imp.line, `raw ${raw[1]} outside the IPC roots: add a channel to @agentos/sdk/host/protocol (zod-validated) instead`)
  }

  if (spec === '@agentos/desktop') report(rel, imp.line, 'nothing imports the host package')
  if (spec.startsWith('@agentos/sdk/testing') && !test && !isWebMode(rel))
    report(rel, imp.line, 'the in-memory host is for tests, stories and web mode only')
  if (spec.startsWith('@agentos/app-') && !under(rel, APP_REGISTRIES) && !test)
    report(rel, imp.line, `apps are wired only in the installedApps registries (${APP_REGISTRIES.join(', ')})`)
  if (rel.startsWith('packages/ui/src/') && spec.startsWith('@agentos/') && !matches(spec, ['@agentos/ui', '@agentos/ui/*']) && !test)
    report(rel, imp.line, `the UI kit depends on no other @agentos package ('${spec}')`)
  if (under(rel, SANDBOXED) && isBuiltin(spec) && !under(rel, NODE_ALLOWED_IN_SANDBOXED) && !test)
    report(rel, imp.line, `'${spec}' is a Node builtin in code that runs sandboxed (no Node)`)
  for (const [zone, foreign] of Object.entries(FOREIGN_SDK_ENTRIES)) {
    if (rel.startsWith(zone) && foreign.includes(spec))
      report(rel, imp.line, `'${spec}' belongs to another process than ${zone}`)
  }
}

function checkCode(rel: string, code: string, markup: string): void {
  const scan = (text: string, re: RegExp, message: string): void => {
    for (const m of text.matchAll(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g')))
      report(rel, lineAt(text, m.index ?? 0), message)
  }

  if (!under(rel, IPC_ROOTS)) scan(code, /\bwebContents\s*\.\s*send\s*\(/, 'raw webContents.send: emit through createHostEmitter and a protocol event channel')
  if (!rel.startsWith('packages/sdk/src/')) scan(code, /\b(?:window|globalThis)\s*\.\s*agentos\b/, 'window.agentos is the host bridge: apps use useAppApi(), the host uses hostBridge()')
  if (!under(rel, HTML_ALLOWED)) {
    const untrusted = 'renders raw HTML; agent, tool and user text is untrusted: build nodes (see atoms/RichText.svelte)'
    scan(markup, /\{@html\b/, `{@html} ${untrusted}`)
    scan(code, /\.(?:innerHTML|outerHTML)\s*\+?=|\binsertAdjacentHTML\s*\(|\bdocument\s*\.\s*write(?:ln)?\s*\(/, `this ${untrusted}`)
  }
  if (rel.startsWith('apps/desktop/src/main/'))
    for (const [re, setting] of WEAK_WEB_PREFERENCES) scan(code, re, `${setting} weakens the renderer sandbox: the renderer stays sandboxed with no Node`)
}

let checked = 0
for (const pattern of SOURCE_GLOBS) {
  for await (const rel of new Glob(pattern).scan({ cwd: ROOT, dot: true })) {
    if (SKIP.test(rel)) continue
    checked++
    const abs = join(ROOT, rel)
    const { code, markup } = split(rel, await Bun.file(abs).text())
    for (const imp of importsOf(code)) checkImport(rel, imp, abs)
    checkCode(rel, code, markup)
  }
}

if (errors.length) {
  console.error(`✗ Boundary check failed (${errors.length} problem${errors.length > 1 ? 's' : ''}):\n`)
  for (const e of [...new Set(errors)].sort((a, b) => a.localeCompare(b, 'en', { numeric: true }))) console.error('  ' + e)
  console.error('\nSee AGENTS.md → "Boundaries" and apps/README.md → "The SDK is the only contract".')
  process.exit(1)
}
console.log(`✓ Boundary check passed (${checked} files)`)
