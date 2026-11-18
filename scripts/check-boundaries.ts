#!/usr/bin/env bun
/**
 * Enforces the AgentOS package boundaries (AGENTS.md → "Boundaries",
 * apps/README.md → "The SDK is the only contract").
 *
 * Rules
 *  1. App modules (apps/<id> other than desktop and server) are built into bundles:
 *     - src/client (browser, shares the host's runtime) imports only APP_CLIENT_IMPORTS + its own files;
 *     - src/server (Bun) may use bun:*, node:* and npm packages declared in the app's package.json,
 *       never host internals, client-side packages or other apps;
 *     - src/shared is pure, browser-safe logic (no framework, SDK runtime, Node or Bun);
 *     - src/client and src/server never import each other (api.invoke / ctx.handle / ctx.emit);
 *     - apps never touch `window.agentos`; clients use useAppApi().
 *  2. No relative import leaves its workspace package (test support may reach the repo's e2e/).
 *  3. The server (apps/server) never imports app packages: apps arrive only as bundles.
 *  4. Browser code (the desktop client, packages/ui, the universal parts of packages/sdk) imports
 *     no Node or Bun builtins, and no server-only SDK entry points.
 *  5. All client ↔ server traffic is the SDK protocol: `new WebSocket` only in the SDK bridge,
 *     Bun.serve / upgrade only in the server entry.
 *  6. @agentos/sdk/testing only in test support, Storybook and web mode; @agentos/app-* in the
 *     desktop client only in web mode (source loaders); packages/ui imports no other @agentos package;
 *     nothing imports electron (AgentOS is a PWA + Bun server).
 *  7. No {@html}, innerHTML, outerHTML, insertAdjacentHTML or document.write: agent and tool
 *     output is untrusted. HTML_ALLOWED lists the reviewed exceptions.
 *
 * Usage: bun scripts/check-boundaries.ts
 */
import { Glob } from 'bun'
import { builtinModules } from 'node:module'
import { dirname, join, relative, resolve, sep } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const SOURCE_GLOBS = ['apps/*/{src,e2e,.storybook}/**/*.{ts,svelte,js,mjs}', 'packages/*/**/*.{ts,svelte,js,mjs}']
const SKIP = /(^|\/)(node_modules|storybook-static|dist|out)\/|^packages\/[^/]+\/[^/]+\.config\.\w+$/

/** What an app's client code may import: these are shared with the host (one runtime). A new entry is an architecture decision. */
const APP_CLIENT_IMPORTS = [
  '@agentos/sdk',
  '@agentos/sdk/app/client',
  '@agentos/ui',
  '@agentos/ui/*',
  'svelte',
  'svelte/*',
  'zod'
]
/** Never in an app's server code. */
const APP_SERVER_FORBIDDEN = ['svelte', 'svelte/*', '@agentos/ui', '@agentos/ui/*', '@agentos/sdk/app/client', '@agentos/sdk/host/*', '@agentos/sdk/build', '@agentos/sdk/testing']
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

/** Static, reviewed markup only. Never pass agent, tool or user text to these. */
const HTML_ALLOWED = ['packages/ui/src/atoms/Icon.svelte'] // SVG strings from the constant icons map
/** Runs in a browser: no Node or Bun builtins. */
const BROWSER = ['apps/desktop/src/', 'packages/ui/src/', 'packages/sdk/src/']
/** Bun-only parts of the SDK. */
const SDK_BUN_ONLY = ['packages/sdk/src/build/']
/** Server-only SDK entry points that browser code must not import. */
const SERVER_ONLY_SDK = ['@agentos/sdk/host/server', '@agentos/sdk/app/server', '@agentos/sdk/build']
const WEBSOCKET_ALLOWED = ['packages/sdk/src/host/ws-bridge.ts']
const BUN_SERVE_ALLOWED = ['apps/server/src/server.ts']

const under = (rel: string, prefixes: string[]): boolean => prefixes.some((p) => (p.endsWith('/') ? rel.startsWith(p) : rel === p))
const matches = (spec: string, patterns: string[]): boolean =>
  patterns.some((p) => (p.endsWith('/*') ? spec === p.slice(0, -2) || spec.startsWith(p.slice(0, -1)) : spec === p))
const isBuiltin = (spec: string): boolean => spec.startsWith('node:') || spec.startsWith('bun:') || spec === 'bun' || builtinModules.includes(spec.split('/')[0])
const isElectron = (spec: string): boolean => spec === 'electron' || spec.startsWith('electron/')
const packageName = (spec: string): string => (spec.startsWith('@') ? spec.split('/').slice(0, 2).join('/') : spec.split('/')[0])

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
const isAppModule = (app: string | undefined): app is string => !!app && app !== 'desktop' && app !== 'server'

/** Which side of an app module a path is on. */
function appPart(rel: string): 'server' | 'client' | 'shared' | null {
  const part = rel.match(/^apps\/[^/]+\/src\/(server|client|shared)\//)?.[1]
  return part === 'server' || part === 'client' || part === 'shared' ? part : null
}

const declaredDeps = new Map<string, Set<string>>()
async function depsOf(app: string): Promise<Set<string>> {
  let deps = declaredDeps.get(app)
  if (!deps) {
    const pkg = (await Bun.file(join(ROOT, 'apps', app, 'package.json')).json()) as Record<string, Record<string, string> | undefined>
    deps = new Set([...Object.keys(pkg.dependencies ?? {}), ...Object.keys(pkg.peerDependencies ?? {})])
    declaredDeps.set(app, deps)
  }
  return deps
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

async function checkAppImport(rel: string, app: string, imp: Import, abs: string, test: boolean): Promise<void> {
  const { spec } = imp
  if (spec.startsWith('.')) {
    if (test) return
    const from = appPart(rel)
    const to = appPart(relative(ROOT, resolve(dirname(abs), spec)).split(sep).join('/') + '/')
    if (from && to && from !== to && (from === 'shared' || to !== 'shared'))
      report(rel, imp.line, from === 'shared'
        ? `src/shared is pure logic shared by both sides; it may not import src/${to} ('${spec}')`
        : `src/${from} may not import src/${to} ('${spec}'): they run in different places (browser / Bun server); use api.invoke / ctx.handle / ctx.emit and put shared code in src/shared`)
    return
  }
  if (isElectron(spec)) return report(rel, imp.line, 'AgentOS has no Electron: apps reach the platform through ctx (app/server) or useAppApi() (app/client)')
  if (spec === '@agentos/desktop' || spec === '@agentos/server' || spec.startsWith('@agentos/app-'))
    return report(rel, imp.line, `apps never import the host or another app ('${spec}'): call other apps' tools`)
  if (test && (matches(spec, TEST_KIT) || matches(spec, APP_CLIENT_IMPORTS) || isBuiltin(spec) || spec === '@agentos/sdk/app/server')) return

  const part = appPart(rel)
  if (part === 'server') {
    if (matches(spec, APP_SERVER_FORBIDDEN))
      return report(rel, imp.line, `src/server runs in the Bun server: '${spec}' belongs to the client or the host`)
    if (isBuiltin(spec) || matches(spec, ['@agentos/sdk', '@agentos/sdk/app/server', 'zod'])) return
    if (!(await depsOf(app)).has(packageName(spec)))
      report(rel, imp.line, `'${spec}' is not a dependency of apps/${app}: add it to its package.json (it is bundled into server.js)`)
    return
  }
  if (part === 'shared' && (isBuiltin(spec) || matches(spec, ['svelte', 'svelte/*', '@agentos/ui', '@agentos/ui/*', '@agentos/sdk/app/*'])))
    return report(rel, imp.line, `src/shared is pure, browser-safe logic (bun tests run it directly, both sides use it); '${spec}' is not allowed`)
  // client, shared, manifest and anything else ships to the browser
  if (isBuiltin(spec)) return report(rel, imp.line, `'${spec}' is a Node/Bun builtin in code that runs in the browser: move it to src/server`)
  if (spec.startsWith('@agentos/sdk/host/') || spec === '@agentos/sdk/app/server')
    return report(rel, imp.line, `'${spec}' is not for app clients: use @agentos/sdk/app/client`)
  if (!matches(spec, APP_CLIENT_IMPORTS))
    report(rel, imp.line, `'${spec}' is not on the app client allow-list (sdk client, ui, svelte, zod): client bundles share the host's runtime; a new client dependency is an architecture decision (APP_CLIENT_IMPORTS)`)
}

async function checkImport(rel: string, imp: Import, abs: string): Promise<void> {
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

  if (isAppModule(app)) return checkAppImport(rel, app, imp, abs, test)

  if (isElectron(spec)) report(rel, imp.line, 'AgentOS has no Electron (PWA client + Bun server)')
  if (spec === '@agentos/desktop' || spec === '@agentos/server') report(rel, imp.line, `nothing imports '${spec}'`)
  if (spec.startsWith('@agentos/sdk/testing') && !test && !isWebMode(rel))
    report(rel, imp.line, 'the in-memory host is for tests, stories and web mode only')
  if (spec.startsWith('@agentos/app-')) {
    if (app === 'server') report(rel, imp.line, 'the server never imports apps: they are installed as bundles')
    else if (!test && !isWebMode(rel)) report(rel, imp.line, 'the client loads apps as bundles; only web mode registers source loaders for repo apps')
  }
  if (rel.startsWith('packages/ui/src/') && spec.startsWith('@agentos/') && !matches(spec, ['@agentos/ui', '@agentos/ui/*']) && !test)
    report(rel, imp.line, `the UI kit depends on no other @agentos package ('${spec}')`)
  if (under(rel, BROWSER) && !under(rel, SDK_BUN_ONLY) && !test) {
    if (isBuiltin(spec)) report(rel, imp.line, `'${spec}' is a Node/Bun builtin in code that runs in the browser`)
    // Types don't ship: web mode may type server modules it runs in the page.
    if (rel.startsWith('apps/desktop/') && matches(spec, SERVER_ONLY_SDK) && !imp.typeOnly)
      report(rel, imp.line, `'${spec}' is server-only`)
  }
}

function checkCode(rel: string, code: string, markup: string): void {
  const scan = (text: string, re: RegExp, message: string): void => {
    for (const m of text.matchAll(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g')))
      report(rel, lineAt(text, m.index ?? 0), message)
  }
  const test = isTestSupport(rel)

  if (!under(rel, WEBSOCKET_ALLOWED) && !test)
    scan(code, /\bnew\s+WebSocket\s*\(/, 'raw WebSocket: talk to the server through the SDK bridge (createWsHostBridge) and the protocol')
  if (!under(rel, BUN_SERVE_ALLOWED) && !test)
    scan(code, /\bBun\s*\.\s*serve\s*\(|\.upgrade\s*\(\s*request/, 'Bun.serve / upgrade belong to the server entry (apps/server/src/server.ts); apps add capabilities through ctx')
  if (!rel.startsWith('packages/sdk/src/') && !test) scan(code, /\b(?:window|globalThis)\s*\.\s*agentos\b/, 'window.agentos is the host bridge: apps use useAppApi(), the shell uses hostBridge()')
  if (!under(rel, HTML_ALLOWED)) {
    const untrusted = 'renders raw HTML; agent, tool and user text is untrusted: build nodes (see atoms/RichText.svelte)'
    scan(markup, /\{@html\b/, `{@html} ${untrusted}`)
    scan(code, /\.(?:innerHTML|outerHTML)\s*\+?=|\binsertAdjacentHTML\s*\(|\bdocument\s*\.\s*write(?:ln)?\s*\(/, `this ${untrusted}`)
  }
}

let checked = 0
for (const pattern of SOURCE_GLOBS) {
  for await (const rel of new Glob(pattern).scan({ cwd: ROOT, dot: true })) {
    if (SKIP.test(rel)) continue
    checked++
    const abs = join(ROOT, rel)
    const { code, markup } = split(rel, await Bun.file(abs).text())
    for (const imp of importsOf(code)) await checkImport(rel, imp, abs)
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
