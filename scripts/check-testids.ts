#!/usr/bin/env bun
/**
 * Enforces the test-id convention (docs/TESTING.md → "Test ids").
 *
 * Rules
 *  1. In every .svelte component, a raw interactive element must carry a
 *     `data-testid`: <button>, <a>, <input>, <select>, <textarea>, any element
 *     with a click / pointer / key handler, and any element with an
 *     interactive ARIA role.
 *  2. Every @agentos/ui component that renders such an element declares a
 *     `testid` prop, so callers are forced (by svelte-check) to name it.
 *
 * Stories are skipped: they document components and are not shipped.
 *
 * Usage: bun scripts/check-testids.ts
 */
import { Glob } from 'bun'
import { join, resolve } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const SOURCE_GLOBS = ['packages/*/src/**/*.svelte', 'apps/*/src/**/*.svelte', 'client/src/**/*.svelte']

const INTERACTIVE_TAGS = new Set(['button', 'a', 'input', 'select', 'textarea'])
const HANDLER_RE = /(?:^|\s)(?:on(?:click|pointerdown|pointerup|keydown|dblclick)\s*=|\{on(?:click|pointerdown|pointerup|keydown|dblclick)\})/
const ROLE_RE =
  /(?:^|\s)role\s*=\s*["'{]\s*["']?(button|link|switch|slider|tab|menuitem|option|checkbox|radio|dialog|textbox|application|menu|listbox)\b/
const TESTID_RE = /(?:^|\s)data-testid\s*[={]/

interface Tag {
  name: string
  attrs: string
  line: number
}

/** Strips <script> and <style> blocks and HTML comments, keeping line numbers. */
function markupOnly(src: string): string {
  const blank = (m: string): string => m.replace(/[^\n]/g, ' ')
  return src
    .replace(/<script\b[\s\S]*?<\/script>/g, blank)
    .replace(/<style\b[\s\S]*?<\/style>/g, blank)
    .replace(/<!--[\s\S]*?-->/g, blank)
}

/** Yields every opening tag, reading attributes up to the closing `>` while skipping `{…}` expressions and quotes. */
function* openingTags(markup: string): Generator<Tag> {
  const re = /<([a-zA-Z][\w:.-]*)/g
  let m: RegExpExecArray | null
  while ((m = re.exec(markup))) {
    const name = m[1]
    let i = re.lastIndex
    let depth = 0
    let quote: string | null = null
    for (; i < markup.length; i++) {
      const c = markup[i]
      if (quote) {
        if (c === quote) quote = null
        continue
      }
      if (c === '{') depth++
      else if (c === '}') depth--
      else if (depth === 0 && (c === '"' || c === "'")) quote = c
      else if (depth > 0 && (c === '"' || c === "'" || c === '`')) quote = c
      else if (c === '>' && depth === 0) break
    }
    const attrs = markup.slice(re.lastIndex, i)
    const line = markup.slice(0, m.index).split('\n').length
    yield { name, attrs, line }
    re.lastIndex = i
  }
}

function isInteractive(tag: Tag): boolean {
  // <svelte:element> renders a real element; the other svelte: tags don't.
  if (tag.name.startsWith('svelte:') && tag.name !== 'svelte:element') return false
  // Components (PascalCase) are checked through their own `testid` prop.
  if (/^[A-Z]/.test(tag.name)) return false
  return INTERACTIVE_TAGS.has(tag.name) || HANDLER_RE.test(tag.attrs) || ROLE_RE.test(tag.attrs)
}

const errors: string[] = []
let checked = 0

for (const pattern of SOURCE_GLOBS) {
  for await (const rel of new Glob(pattern).scan({ cwd: ROOT })) {
    if (rel.includes('node_modules')) continue
    if (/\.stories\.svelte$/.test(rel)) continue
    checked++
    const src = await Bun.file(join(ROOT, rel)).text()
    let rendersInteractive = false

    for (const tag of openingTags(markupOnly(src))) {
      if (!isInteractive(tag)) continue
      rendersInteractive = true
      if (!TESTID_RE.test(tag.attrs)) {
        errors.push(`${rel}:${tag.line}: <${tag.name}> is interactive but has no data-testid`)
      }
    }

    if (rendersInteractive && rel.startsWith('packages/ui/src/')) {
      const script = src.match(/<script\b(?![^>]*\bmodule\b)[^>]*>([\s\S]*?)<\/script>/)?.[1] ?? ''
      if (!/\btestid\s*:\s*string\b/.test(script)) {
        errors.push(`${rel}: kit component renders interactive elements but declares no required \`testid: string\` prop`)
      }
    }
  }
}

if (errors.length) {
  console.error(`✗ Test id check failed (${errors.length} problem${errors.length > 1 ? 's' : ''}):\n`)
  for (const e of errors) console.error('  ' + e)
  console.error('\nSee docs/TESTING.md → "Test ids".')
  process.exit(1)
}
console.log(`✓ Test id check passed (${checked} components)`)
