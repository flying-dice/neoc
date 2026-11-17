#!/usr/bin/env bun
/**
 * Static design-language audit for one app: flags styles that bypass the
 * tokens in docs/design/DESIGN_LANGUAGE.md. Feeds the styleguide conformance
 * score in the app-design skill (it is evidence, not the whole score).
 *
 *   bun scripts/design-audit.ts <appId> [--json]
 *   bun scripts/design-audit.ts --all
 *
 * Checks every <style> block and style: directive in the app's components:
 *   colour      raw #hex / rgb() / hsl() instead of a colour token
 *   type        raw font sizes instead of --type-* tokens
 *   radius      raw border-radius instead of --radius-* (999px, 50%, inherit ok)
 *   shadow      box-shadow not from --shadow-*
 *   gradient    linear/radial gradients (flat design: wallpaper and icon tiles only)
 *   material    backdrop-filter without --material-blur
 *   textshadow  text-shadow
 *   spacing     padding / margin / gap off the 4pt grid
 *   emoji       emoji in markup
 *
 * A deliberate exception is allowed on a line ending with
 * `/* design-exception: <reason> *\/` (e.g. the Calculator's instrument palette).
 */
import { Glob } from 'bun'
import { dirname, join, relative, resolve } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const DESKTOP = join(ROOT, 'apps/desktop/src/renderer/src')

/** Built-in apps → their page component (pages pull in their own components). */
const BUILTIN_PAGES: Record<string, string> = {
  desktop: 'DesktopPage.svelte',
  discover: 'DiscoverPage.svelte',
  agents: 'AgentsPage.svelte',
  settings: 'SettingsPage.svelte',
  files: 'FilesPage.svelte',
  gallery: 'GalleryPage.svelte',
  terminal: 'TerminalPage.svelte'
}

interface Finding {
  rule: string
  file: string
  line: number
  text: string
}

async function filesFor(app: string): Promise<string[]> {
  if (BUILTIN_PAGES[app]) {
    const seen = new Set<string>()
    const visit = async (file: string): Promise<void> => {
      // `x.svelte` can also be a `x.svelte.ts` rune module; only components are audited.
      if (seen.has(file) || !(await Bun.file(file).exists())) return
      seen.add(file)
      const src = await Bun.file(file).text()
      for (const m of src.matchAll(/from\s+['"](\.{1,2}\/[^'"]+\.svelte)['"]/g)) await visit(resolve(dirname(file), m[1]))
    }
    await visit(join(DESKTOP, 'pages', BUILTIN_PAGES[app]))
    return [...seen]
  }
  const dir = join(ROOT, 'apps', app, 'src/renderer')
  const files: string[] = []
  for await (const f of new Glob('**/*.svelte').scan({ cwd: dir })) if (!f.includes('.stories.')) files.push(join(dir, f))
  if (!files.length) throw new Error(`Unknown app "${app}". Built-in: ${Object.keys(BUILTIN_PAGES).join(', ')}; or a folder in apps/.`)
  return files
}

const RULES: { rule: string; test: (line: string) => boolean }[] = [
  { rule: 'colour', test: (l) => /(#[0-9a-f]{3,8}\b|\brgba?\(|\bhsla?\()/i.test(l.replace(/var\([^)]*\)/g, '')) && !/^\s*--/.test(l) },
  { rule: 'type', test: (l) => /\bfont(-size)?\s*:[^;]*\d+(\.\d+)?px/.test(l) && !/var\(--type/.test(l) },
  {
    rule: 'radius',
    test: (l) => /border(-[a-z-]+)?-radius\s*:/.test(l) && /\d+px/.test(l) && !/var\(--radius|999px|50%|inherit/.test(l)
  },
  { rule: 'shadow', test: (l) => /box-shadow\s*:/.test(l) && !/var\(--shadow|none/.test(l) },
  { rule: 'gradient', test: (l) => /(linear|radial|conic)-gradient\(/.test(l) },
  { rule: 'material', test: (l) => /backdrop-filter\s*:/.test(l) && !/var\(--material-blur\)/.test(l) },
  { rule: 'textshadow', test: (l) => /text-shadow\s*:/.test(l) && !/none/.test(l) },
  {
    rule: 'spacing',
    test: (l) => {
      const m = l.match(/^\s*(padding|margin|gap|row-gap|column-gap)(-[a-z]+)?\s*:\s*([^;]+)/)
      if (!m) return false
      return [...m[3].matchAll(/(\d+(?:\.\d+)?)px/g)].some(([, n]) => Number(n) > 2 && Number(n) % 4 !== 0)
    }
  }
]

async function audit(app: string): Promise<{ app: string; files: string[]; counts: Record<string, number>; findings: Finding[] }> {
  const files = await filesFor(app)
  const findings: Finding[] = []
  for (const file of files) {
    const rel = relative(ROOT, file)
    const lines = (await Bun.file(file).text()).split('\n')
    let inStyle = false
    lines.forEach((text, i) => {
      if (/<style/.test(text)) inStyle = true
      if (/<\/style>/.test(text)) inStyle = false
      if (/design-exception:/.test(text)) return
      const styleDirective = /\bstyle(:[a-z-]+)?=/.test(text)
      if (inStyle || styleDirective) {
        for (const r of RULES) if (r.test(text)) findings.push({ rule: r.rule, file: rel, line: i + 1, text: text.trim() })
      } else if (/\p{Extended_Pictographic}/u.test(text) && !/^\s*(\/\/|\*|<!--)/.test(text)) {
        findings.push({ rule: 'emoji', file: rel, line: i + 1, text: text.trim() })
      }
    })
  }
  const counts: Record<string, number> = {}
  for (const f of findings) counts[f.rule] = (counts[f.rule] ?? 0) + 1
  return { app, files: files.map((f) => relative(ROOT, f)), counts, findings }
}

const args = process.argv.slice(2)
const json = args.includes('--json')
const apps = args.includes('--all')
  ? [...Object.keys(BUILTIN_PAGES), ...(await Array.fromAsync(new Glob('apps/*/src/manifest.ts').scan({ cwd: ROOT }))).map((p) => p.split('/')[1])]
  : args.filter((a) => !a.startsWith('--'))
if (!apps.length) {
  console.error('Usage: bun scripts/design-audit.ts <appId> [--json] | --all')
  process.exit(1)
}

const results = await Promise.all(apps.map(audit))
if (json) console.log(JSON.stringify(results.length === 1 ? results[0] : results, null, 2))
else
  for (const r of results) {
    const total = r.findings.length
    console.log(`\n${r.app}: ${total} finding${total === 1 ? '' : 's'} in ${r.files.length} files ${JSON.stringify(r.counts)}`)
    for (const f of r.findings) console.log(`  ${f.rule.padEnd(10)} ${f.file}:${f.line}  ${f.text.slice(0, 100)}`)
  }
