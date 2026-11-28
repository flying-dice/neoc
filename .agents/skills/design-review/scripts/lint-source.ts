#!/usr/bin/env bun
/**
 * Read a surface's source for what the pictures cannot prove: styles written by hand instead of from the design
 * tokens, controls hand-rolled instead of the kit's, browser dialogs, and copy that breaks the writing rules.
 *
 *   bun .agents/skills/design-review/scripts/lint-source.ts <folder> [--out <file>]
 *
 * <folder> is an app (`apps/notes`), a package (`packages/ui`) or `client`. Scans src/**\/*.{svelte,ts,css} except
 * tests and stories. Prints a table and writes it to <out> (default test-results/design-review/<name>/source.md).
 * Every line is a lead, not a verdict: a hand-written value can be right (a token cannot say it); say why or fix it.
 */
import { mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { basename, dirname, join, relative, resolve } from 'node:path'
import { Glob } from 'bun'

const ROOT = resolve(import.meta.dir, '../../../..')
const args = process.argv.slice(2)
const folder = args.find((a, i) => !a.startsWith('--') && args[i - 1] !== '--out')
if (!folder) {
  console.error('usage: lint-source.ts <folder> [--out <file>]')
  process.exit(2)
}
const base = resolve(ROOT, folder)
const isApp = relative(ROOT, base).replaceAll('\\', '/').startsWith('apps/')
const out = resolve(args.includes('--out') ? args[args.indexOf('--out') + 1] : join(ROOT, 'test-results', 'design-review', basename(base), 'source.md'))

interface Lead {
  check: string
  file: string
  line: number
  text: string
  why: string
}
const leads: Lead[] = []

// a declaration whose value must come from a token; the value may still hold var(...) beside a literal
const DECLARATIONS: { prop: RegExp; check: string; ok: RegExp; why: string }[] = [
  { prop: /^font$/, check: 'src-font', ok: /^(var\(--type-[\w-]+\)|inherit)$/, why: 'the font shorthand is a --type-* token' },
  { prop: /^font-size$/, check: 'src-font-size', ok: /^(inherit|var\(--[\w-]+\)|calc\([^)]*var\(--text-scale\)[^)]*\)|\d+(\.\d+)?em|100%)$/, why: 'a text size comes from the type ramp (--type-*, or an .ios-* class), so it follows Dynamic Type' },
  { prop: /^font-family$/, check: 'src-font-family', ok: /^(var\(--font-(text|display|rounded|mono)\)|inherit)$/, why: 'a family is --font-text, --font-display, --font-rounded or --font-mono' },
  { prop: /^font-weight$/, check: 'src-font-weight', ok: /^(400|500|600|700|normal|bold|inherit)$/, why: 'weights are regular 400, medium 500, semibold 600, bold 700' },
  { prop: /^letter-spacing$/, check: 'src-letter-spacing', ok: /^(var\(--tracking-[\w-]+\)|inherit|0|normal)$/, why: 'tracking is the --tracking-* of the text size' },
  { prop: /^line-height$/, check: 'src-line-height', ok: /^(inherit|1|normal|var\(--[\w-]+\))$/, why: 'line height comes with the --type-* token' },
  { prop: /^border-radius$/, check: 'src-radius', ok: /^(var\(--radius-[\w-]+\)|50%|0|inherit)$/, why: 'a radius is a --radius-* token' },
  { prop: /^box-shadow$/, check: 'src-shadow', ok: /^(var\(--shadow-[\w-]+\)|none|inherit)$/, why: 'elevation is a --shadow-* token' },
  { prop: /^(transition|animation)(-duration|-timing-function)?$/, check: 'src-motion', ok: /^(none|inherit|0s?)$|var\(--(duration|ease)/, why: 'durations and easing are --duration-* and --ease-* (Reduce Motion is handled for them)' },
  { prop: /^text-transform$/, check: 'src-uppercase', ok: /^(none|inherit|capitalize)$/, why: 'body text is never set in capitals; check the rule for the element' },
  { prop: /^font-style$/, check: 'src-italic', ok: /^(normal|inherit)$/, why: 'italic is not a UI emphasis on iPadOS' }
]
const COLOR_LITERAL = /#[0-9a-fA-F]{3,8}\b|\b(rgba?|hsla?|hwb|lab|lch|oklab|oklch)\(|\b(white|black|red|green|blue|gray|grey|silver|orange|yellow|purple|pink)\b/
const COLOR_PROPS = /^(color|background|background-color|border|border-(top|right|bottom|left)(-color)?|border-color|outline|outline-color|fill|stroke|caret-color|text-decoration-color|accent-color)$/

function scanStyle(file: string, css: string, offset: number): void {
  const re = /([a-z-]+)\s*:\s*([^;{}]+);/g
  for (let m = re.exec(css); m; m = re.exec(css)) {
    const [, prop, raw] = m
    const value = raw.trim().replace(/\s*!important$/, '')
    const line = offset + css.slice(0, m.index).split('\n').length - 1
    if (raw.includes('!important')) leads.push({ check: 'src-important', file, line, text: m[0].trim(), why: '!important fights the design system' })
    if (prop.startsWith('--')) continue
    for (const d of DECLARATIONS) {
      if (d.prop.test(prop) && !d.ok.test(value)) leads.push({ check: d.check, file, line, text: m[0].trim(), why: d.why })
    }
    if (COLOR_PROPS.test(prop) && COLOR_LITERAL.test(value.replace(/var\([^)]*\)/g, '')) && !/transparent|currentColor/i.test(value.replace(COLOR_LITERAL, ''))) {
      leads.push({ check: 'src-color-literal', file, line, text: m[0].trim(), why: 'a colour is a token (--label*, --bg*, --fill*, --separator*, --tint, --ios-*), so it follows the theme' })
    }
  }
}

// copy: text in markup and quoted strings that a person reads
const COPY: { re: RegExp; check: string; why: string }[] = [
  { re: /\.\.\.(?!\w)/, check: 'copy-ellipsis', why: 'an ellipsis is one character (…)' },
  { re: /\b(Click|Tap) here\b/i, check: 'copy-click-here', why: 'name the action, never "click here"' },
  { re: /\bPlease\b/, check: 'copy-please', why: 'iPadOS copy is direct; "please" is padding' },
  { re: /\b(Oops|Whoops|Uh oh)\b/i, check: 'copy-tone', why: 'no cute or alarming words in errors' },
  { re: /\b(Error|Warning|Failed):/, check: 'copy-label-prefix', why: 'say what happened and what to do, not "Error:"' },
  { re: /\b(Invalid|Illegal|Fatal|Exception|Null|undefined|NaN)\b/, check: 'copy-jargon', why: 'developer words never reach a person' },
  { re: /[a-z][!]\s*['"`<]/, check: 'copy-exclamation', why: 'no exclamation marks in UI copy' },
  { re: />\s*OK\s*</, check: 'copy-ok', why: 'a button says what it does ("Delete", "Save"); "OK" only acknowledges' },
  { re: /\b[A-Z]{4,}\b(?![-_])/, check: 'copy-all-caps', why: 'no words in capitals for emphasis (acronyms aside)' }
]

for await (const rel of new Glob('src/**/*.{svelte,ts,css}').scan({ cwd: base })) {
  if (/\.(test|stories)\.|story-support|\.d\.ts$/.test(rel)) continue
  const file = join(base, rel)
  const shown = relative(ROOT, file).replaceAll('\\', '/')
  const source = readFileSync(file, 'utf8')
  if (rel.endsWith('.css')) scanStyle(shown, source, 1)
  // a Svelte <style>, a style="" attribute, and a Lit css`` template
  for (const m of source.matchAll(/<style[^>]*>([\s\S]*?)<\/style>|css`([\s\S]*?)`/g)) {
    const css = m[1] ?? m[2] ?? ''
    scanStyle(shown, css, source.slice(0, m.index).split('\n').length)
  }
  for (const m of source.matchAll(/style="([^"]*)"/g)) scanStyle(shown, m[1] + ';', source.slice(0, m.index).split('\n').length)
  source.split('\n').forEach((text, i) => {
    const line = i + 1
    if (/\b(window\.)?(alert|confirm|prompt)\(/.test(text) && !/\/\/|\*/.test(text.trim().slice(0, 2))) leads.push({ check: 'src-native-dialog', file: shown, line, text: text.trim(), why: 'a browser dialog is never iPadOS: use ui-alert' })
    if (isApp && /<(button|input|select|textarea)\b/.test(text)) leads.push({ check: 'src-raw-control', file: shown, line, text: text.trim(), why: 'an app uses the kit (ui-button, ui-text-field, ui-picker, ui-text-area, ui-toggle), not native controls it styles itself' })
    if (/\btitle="[^"{]/.test(text)) leads.push({ check: 'src-title-tooltip', file: shown, line, text: text.trim(), why: 'a title="" tooltip is help on hover only; is the same said where touch can reach it?' })
    if (/^\s*(import|\/\/|\*|\/\*)/.test(text)) return
    const strings = [...text.matchAll(/>([^<>{}]+)</g), ...text.matchAll(/(?:label|title|header|footer|message|placeholder|heading|text|description)=["']([^"']+)["']/g), ...text.matchAll(/['"`]([A-Z][^'"`]{3,})['"`]/g)].map((m) => m[1])
    for (const s of strings) {
      for (const c of COPY) {
        if (c.re.test(c.check === 'copy-ok' || c.check === 'copy-exclamation' ? text : s)) {
          leads.push({ check: c.check, file: shown, line, text: s.trim().slice(0, 100), why: c.why })
          break
        }
      }
    }
  })
}

const byCheck = new Map<string, Lead[]>()
for (const l of leads) byCheck.set(l.check, [...(byCheck.get(l.check) ?? []), l])
const lines = [`# Source leads: ${relative(ROOT, base).replaceAll('\\', '/')}`, '', `${leads.length} leads. Each is a place to look, not a verdict.`, '']
for (const [check, list] of [...byCheck.entries()].sort((a, b) => b[1].length - a[1].length)) {
  lines.push(`## ${check} (${list.length})`, '', `_${list[0].why}_`, '')
  for (const l of list.slice(0, 80)) lines.push(`- \`${l.file}:${l.line}\` ${'`' + l.text.replaceAll('`', "'").slice(0, 140) + '`'}`)
  if (list.length > 80) lines.push(`- … ${list.length - 80} more`)
  lines.push('')
}
mkdirSync(dirname(out), { recursive: true })
writeFileSync(out, lines.join('\n'))
console.log(`${leads.length} leads in ${byCheck.size} checks: ${relative(ROOT, out)}`)
for (const [check, list] of byCheck) console.log(`  ${check}: ${list.length}`)
