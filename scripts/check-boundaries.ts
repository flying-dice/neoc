#!/usr/bin/env bun
/**
 * Enforces the TypeScript boundaries that are left now that apps are Luau and the host is Rust
 * (AGENTS.md -> "Boundaries"). The TypeScript here is the client (client/), the UI kit (packages/ui), the
 * shared Storybook setup (packages/storybook) and the specs.
 *
 * Rules
 *  1. No relative import leaves its workspace package (client, packages/<name>, apps/<name>): the client never
 *     reaches host internals (crates/); the one exception is the types generated from the Rust schema
 *     (types/generated), which the renderer's protocol.ts re-exports. Test support may reach the repo's e2e/.
 *  2. The UI kit imports no other @agentos package; the client imports only @agentos/ui (and, in stories,
 *     @agentos/storybook).
 *  3. Browser code (client/src, packages/ui/src) imports no Node or Bun builtins.
 *  4. Client <-> host traffic is JSON-RPC over /rpc: `new WebSocket` only in the renderer's transport.
 *  5. No {@html}, innerHTML, outerHTML, insertAdjacentHTML or document.write: agent and tool
 *     output is untrusted. HTML_ALLOWED lists the reviewed exceptions.
 *
 * Usage: bun scripts/check-boundaries.ts
 */
import { Glob } from 'bun'
import { builtinModules } from 'node:module'
import { dirname, join, relative, resolve, sep } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const SOURCE_GLOBS = [
  'apps/*/e2e/**/*.{ts,svelte,js,mjs}',
  'client/{src,e2e,.storybook}/**/*.{ts,svelte,js,mjs}',
  'packages/*/**/*.{ts,svelte,js,mjs}'
]
const SKIP = /(^|\/)(node_modules|storybook-static|dist|out)\/|^packages\/[^/]+\/[^/]+\.config\.\w+$/

/** The renderer's protocol.ts re-exports the types generated from the Rust schema: the one file that reaches types/. */
const GENERATED_REEXPORT = 'client/src/renderer/src/view/protocol.ts'
/** Static, reviewed markup only. Never pass agent, tool or user text to these. */
const HTML_ALLOWED = ['packages/ui/src/atoms/Icon.svelte'] // SVG strings from the constant icons map
/** Runs in a browser: no Node or Bun builtins. */
const BROWSER = ['client/src/', 'packages/ui/src/']
const WEBSOCKET_ALLOWED = ['client/src/renderer/src/view/rpc/websocket.ts'] // the reconnecting JSON-RPC transport

const under = (rel: string, prefixes: string[]): boolean => prefixes.some((p) => (p.endsWith('/') ? rel.startsWith(p) : rel === p))
const matches = (spec: string, patterns: string[]): boolean =>
  patterns.some((p) => (p.endsWith('/*') ? spec === p.slice(0, -2) || spec.startsWith(p.slice(0, -1)) : spec === p))
const isBuiltin = (spec: string): boolean => spec.startsWith('node:') || spec.startsWith('bun:') || spec === 'bun' || builtinModules.includes(spec.split('/')[0])

function isTestSupport(rel: string): boolean {
  return (
    /\.(test|spec)\.ts$/.test(rel) ||
    /\.stories\.(svelte|ts)$/.test(rel) ||
    /^((apps|packages)\/[^/]+|client)\/(e2e|\.storybook)\//.test(rel) ||
    rel.startsWith('packages/storybook/')
  )
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
  line: number
}

const STATIC_IMPORT = /\b(?:import|export)\s+(?:type\s+)?(?:[\w$*{}\s,]+?\s+from\s+)?['"]([^'"\n]+)['"]/g
const DYNAMIC_IMPORT = /\b(?:import|require)\s*\(\s*['"]([^'"\n]+)['"]\s*\)/g

function importsOf(code: string): Import[] {
  const found: Import[] = []
  for (const m of code.matchAll(STATIC_IMPORT))
    found.push({ spec: m[1], line: lineAt(code, m.index ?? 0) })
  for (const m of code.matchAll(DYNAMIC_IMPORT))
    found.push({ spec: m[1], line: lineAt(code, m.index ?? 0) })
  return found
}

// ── Rules ────────────────────────────────────────────────────────────────

const errors: string[] = []
const report = (rel: string, line: number, message: string): void => void errors.push(`${rel}:${line}: ${message}`)

const workspaceOf = (rel: string): string => (rel.startsWith('client/') ? 'client' : rel.split('/').slice(0, 2).join('/'))

function checkRelative(rel: string, imp: Import, abs: string, test: boolean): void {
  const pkg = workspaceOf(rel)
  const target = relative(ROOT, resolve(dirname(abs), imp.spec)).split(sep).join('/')
  const escapes = target !== pkg && !target.startsWith(pkg + '/')
  const generated = rel === GENERATED_REEXPORT && target.startsWith('types/generated/')
  if (escapes && !generated && !(test && target.startsWith('e2e/')))
    report(rel, imp.line, `relative import '${imp.spec}' leaves ${pkg}: cross packages only through their published entry points`)
}

function checkAgentosImport(rel: string, imp: Import, test: boolean): void {
  const { spec } = imp
  const kit = matches(spec, ['@agentos/ui', '@agentos/ui/*'])
  const storybook = test && matches(spec, ['@agentos/storybook', '@agentos/storybook/*'])
  if (rel.startsWith('packages/ui/src/') && !test) return report(rel, imp.line, `the UI kit depends on no other @agentos package ('${spec}')`)
  if (!kit && !storybook && !rel.startsWith('packages/storybook/'))
    report(rel, imp.line, `'${spec}' is not importable here: the client uses only @agentos/ui (stories also @agentos/storybook)`)
}

function checkImport(rel: string, imp: Import, abs: string): void {
  const { spec } = imp
  const test = isTestSupport(rel)
  if (spec.startsWith('.')) return checkRelative(rel, imp, abs, test)
  if (spec.startsWith('@agentos/')) checkAgentosImport(rel, imp, test)
  if (under(rel, BROWSER) && !test && isBuiltin(spec)) report(rel, imp.line, `'${spec}' is a Node/Bun builtin in code that runs in the browser`)
}

function checkCode(rel: string, code: string, markup: string): void {
  const scan = (text: string, re: RegExp, message: string): void => {
    for (const m of text.matchAll(new RegExp(re.source, re.flags.includes('g') ? re.flags : re.flags + 'g')))
      report(rel, lineAt(text, m.index ?? 0), message)
  }
  if (!under(rel, WEBSOCKET_ALLOWED) && !isTestSupport(rel))
    scan(code, /\bnew\s+WebSocket\s*\(/, 'raw WebSocket: talk to the host through the renderer transport (view/rpc/websocket.ts)')
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
    for (const imp of importsOf(code)) checkImport(rel, imp, abs)
    checkCode(rel, code, markup)
  }
}

if (errors.length) {
  console.error(`✗ Boundary check failed (${errors.length} problem${errors.length > 1 ? 's' : ''}):\n`)
  for (const e of [...new Set(errors)].sort((a, b) => a.localeCompare(b, 'en', { numeric: true }))) console.error('  ' + e)
  console.error('\nSee AGENTS.md -> "Boundaries".')
  process.exit(1)
}
console.log(`✓ Boundary check passed (${checked} files)`)
