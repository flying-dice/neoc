#!/usr/bin/env bun
/**
 * Enforces strict atomic design across the workspace.
 *
 *   atoms < molecules < organisms < templates < pages
 *
 * Rules
 *  1. Every .svelte component lives in exactly one layer folder
 *     (atoms/ molecules/ organisms/ templates/ pages/). The only exceptions
 *     are the composition roots (`isCompositionRoot`).
 *  2. A component may import components only from STRICTLY LOWER layers —
 *     never from its own layer or above. Atoms import no components at all.
 *  3. Each component starts with a `<!-- @<layer> Name — … -->` header that
 *     matches its folder, so the layer is visible when reading the file.
 *  4. Components from @vos/ui are imported through their layer entry
 *     point (`@vos/ui/atoms`, …), never by deep path.
 *
 * Usage: bun scripts/check-atomic.ts
 */
import { Glob } from 'bun'
import { dirname, join, resolve, sep } from 'node:path'

const ROOT = resolve(import.meta.dir, '..')
const LAYERS = ['atoms', 'molecules', 'organisms', 'templates', 'pages'] as const
type Layer = (typeof LAYERS)[number]
const rank = (l: Layer): number => LAYERS.indexOf(l)

const SOURCE_GLOBS = ['packages/*/src/**/*.{svelte,ts}', 'apps/*/src/**/*.{svelte,ts}', 'client/src/**/*.{svelte,ts}']

/** Files allowed outside the layer folders: the entry point that wires pages together, the client's and each app's. */
const isCompositionRoot = (rel: string): boolean => rel === 'client/src/App.svelte' || /^apps\/[^/]+\/src\/App\.svelte$/.test(rel)

function layerOfPath(path: string): Layer | null {
  const parts = path.split(sep)
  // Use the innermost layer segment.
  for (let i = parts.length - 2; i >= 0; i--) {
    if ((LAYERS as readonly string[]).includes(parts[i])) return parts[i] as Layer
  }
  return null
}

function layerOfSpecifier(spec: string, fromFile: string): Layer | 'deep-ui' | null {
  const ui = spec.match(/^@vos\/ui(?:\/(.*))?$/)
  if (ui) {
    const sub = ui[1]
    if (!sub) return null
    if ((LAYERS as readonly string[]).includes(sub)) return sub as Layer
    if (sub.endsWith('.css')) return null
    return 'deep-ui'
  }
  if (spec.startsWith('.')) {
    if (!/\.svelte$|\/(atoms|molecules|organisms|templates|pages)(\/index(\.ts)?)?$/.test(spec)) {
      return null // non-component module (types, icons, stores, logic)
    }
    return layerOfPath(resolve(dirname(fromFile), spec) + (spec.endsWith('.svelte') ? '' : sep + 'x'))
  }
  return null
}

const IMPORT_RE = /(?:import|export)\s+(?:type\s+)?(?:[^'"]*?\sfrom\s+)?['"]([^'"]+)['"]|import\(\s*['"]([^'"]+)['"]\s*\)/g

const errors: string[] = []
let checked = 0

for (const pattern of SOURCE_GLOBS) {
  for await (const rel of new Glob(pattern).scan({ cwd: ROOT })) {
    if (rel.includes('node_modules')) continue
    // Stories document components; they may compose anything and carry no layer.
    if (/\.stories\.(svelte|ts)$/.test(rel)) continue
    const abs = join(ROOT, rel)
    const isSvelte = rel.endsWith('.svelte')
    const isBarrel = /\/(atoms|molecules|organisms|templates|pages)\/index\.ts$/.test(rel)
    const layer = layerOfPath(rel)

    if (isSvelte && !layer && !isCompositionRoot(rel)) {
      errors.push(`${rel}: component is not inside an atomic layer folder (${LAYERS.join(', ')})`)
      continue
    }
    if (!layer || isBarrel) continue
    checked++

    const src = await Bun.file(abs).text()

    if (isSvelte) {
      const header = src.match(/^\s*<!--\s*@(\w+)/)
      if (!header) errors.push(`${rel}: missing <!-- @${layer} … --> header comment`)
      else if (header[1] !== layer.replace(/s$/, ''))
        errors.push(`${rel}: header says @${header[1]} but file lives in ${layer}/`)
    }

    for (const m of src.matchAll(IMPORT_RE)) {
      const spec = m[1] ?? m[2]
      const target = layerOfSpecifier(spec, abs)
      if (target === null) continue
      if (target === 'deep-ui') {
        errors.push(`${rel}: deep import '${spec}' — use a layer entry point like '@vos/ui/atoms'`)
        continue
      }
      if (rank(target) >= rank(layer)) {
        errors.push(
          `${rel}: ${layer.replace(/s$/, '')} may not import from ${target} ('${spec}') — only strictly lower layers`
        )
      }
    }
  }
}

if (errors.length) {
  console.error(`✗ Atomic design check failed (${errors.length} problem${errors.length > 1 ? 's' : ''}):\n`)
  for (const e of errors) console.error('  ' + e)
  process.exit(1)
}
console.log(`✓ Atomic design check passed (${checked} files, layers: ${LAYERS.join(' < ')})`)
