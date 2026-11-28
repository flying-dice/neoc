#!/usr/bin/env bun
/**
 * Fetch Apple's Human Interface Guidelines, the pages an iPadOS review needs, as plain markdown: read live from Apple
 * (its DocC JSON), never copied into the repository.
 *
 *   bun .agents/skills/design-review/scripts/fetch-hig.ts [page ...]
 *
 * With no pages, every page in PAGES. Written to test-results/design-review/hig/<page>.md (ignored by git); a page
 * fetched in the last day is kept.
 */
import { existsSync, mkdirSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
const ROOT = resolve(import.meta.dir, '../../../..')
const OUT = join(ROOT, 'test-results', 'design-review', 'hig')
export const PAGES = ['typography', 'writing', 'color', 'dark-mode', 'layout', 'materials', 'accessibility', 'sf-symbols', 'icons', 'images', 'labels', 'buttons', 'toggles', 'segmented-controls', 'text-fields', 'pickers', 'sliders', 'steppers', 'pull-down-buttons', 'pop-up-buttons', 'disclosure-controls', 'page-controls', 'menus', 'context-menus', 'edit-menus', 'alerts', 'action-sheets', 'sheets', 'popovers', 'modality', 'sidebars', 'tab-bars', 'toolbars', 'split-views', 'search-fields', 'lists-and-tables', 'collections', 'scroll-views', 'progress-indicators', 'loading', 'feedback', 'entering-data', 'onboarding', 'offering-help', 'launching', 'notifications', 'settings', 'keyboards', 'virtual-keyboards', 'pointing-devices', 'drag-and-drop', 'multitasking', 'windows', 'going-full-screen', 'undo-and-redo', 'motion', 'gauges', 'boxes', 'rating-indicators', 'color-wells', 'combo-boxes']
const asked = process.argv.slice(2)
mkdirSync(OUT, { recursive: true })
type Node = any
function inline(nodes: Node[] | undefined, refs: Record<string, any>): string {
  return (nodes ?? []).map((n) => {
    switch (n.type) {
      case 'text': return n.text
      case 'codeVoice': return '`' + n.code + '`'
      case 'emphasis': return '*' + inline(n.inlineContent, refs) + '*'
      case 'strong': return '**' + inline(n.inlineContent, refs) + '**'
      case 'reference': return refs[n.identifier]?.title ?? ''
      case 'image': return ''
      default: return n.inlineContent ? inline(n.inlineContent, refs) : (n.text ?? '')
    }
  }).join('')
}
function block(nodes: Node[] | undefined, refs: Record<string, any>, depth = 0): string[] {
  const out: string[] = []
  for (const n of nodes ?? []) {
    switch (n.type) {
      case 'heading': out.push('', '#'.repeat(Math.min(6, (n.level ?? 2) + 1)) + ' ' + n.text, ''); break
      case 'paragraph': out.push(inline(n.inlineContent, refs), ''); break
      case 'unorderedList': case 'orderedList':
        for (const item of n.items ?? []) out.push('  '.repeat(depth) + '- ' + block(item.content, refs, depth + 1).filter(Boolean).join(' '))
        out.push(''); break
      case 'aside': out.push('> ' + (n.name ?? n.style ?? 'Note') + ': ' + block(n.content, refs, depth).filter(Boolean).join(' '), ''); break
      case 'table':
        for (const row of n.rows ?? []) out.push('| ' + row.map((cell: Node[]) => block(cell, refs, depth).filter(Boolean).join(' ')).join(' | ') + ' |')
        out.push(''); break
      case 'row': for (const c of n.columns ?? []) out.push(...block(c.content, refs, depth)); break
      case 'tabNavigator': for (const t of n.tabs ?? []) { out.push('', '**' + t.title + '**', ''); out.push(...block(t.content, refs, depth)) } break
      case 'links': out.push('Related: ' + (n.items ?? []).map((i: string) => refs[i]?.title ?? '').join(', '), ''); break
      default: if (n.content) out.push(...block(n.content, refs, depth)); else if (n.inlineContent) out.push(inline(n.inlineContent, refs), '')
    }
  }
  return out
}
for (const slug of asked.length ? asked : PAGES) {
  const file = join(OUT, `${slug}.md`)
  if (existsSync(file) && Date.now() - statSync(file).mtimeMs < 86_400_000) continue
  const url = `https://developer.apple.com/tutorials/data/design/human-interface-guidelines/${slug}.json`
  const r = await fetch(url)
  if (!r.ok) { console.log(`skip ${slug}: ${r.status}`); continue }
  const j: any = await r.json()
  const refs = j.references ?? {}
  const lines = [`# ${j.metadata?.title ?? slug}`, '', inline(j.abstract, refs), '']
  for (const s of j.primaryContentSections ?? []) lines.push(...block(s.content, refs))
  const text = lines.join('\n').replace(/\n{3,}/g, '\n\n')
  writeFileSync(file, text)
  console.log(`${slug}: ${text.length}`)
}
