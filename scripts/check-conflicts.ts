#!/usr/bin/env bun
/**
 * Fails on leftover merge-conflict markers in tracked text files.
 *
 * A conflict is a block: a line starting `<<<<<<< `, then `=======` alone on its line (exactly seven), then a
 * line starting `>>>>>>> `. The separator and the closing marker only count inside a block, so a setext
 * heading underline (`=====` under a title) or a stray `=======` never trips it. Binary files are skipped.
 *
 * Usage: bun scripts/check-conflicts.ts
 */
import { resolve } from 'node:path'

export interface Marker {
  line: number
  text: string
}

const OPEN = '<<<<<<< '
const SEPARATOR = '======='
const CLOSE = '>>>>>>> '

/** The marker lines of every conflict block in `text` (1-based line numbers). An unclosed block reports to the end. */
export function findConflictMarkers(text: string): Marker[] {
  const found: Marker[] = []
  let inBlock = false
  text.split(/\r?\n/).forEach((line, index) => {
    const isMarker = line.startsWith(OPEN) || (inBlock && (line === SEPARATOR || line.startsWith(CLOSE)))
    if (line.startsWith(OPEN)) inBlock = true
    else if (inBlock && line.startsWith(CLOSE)) inBlock = false
    if (isMarker) found.push({ line: index + 1, text: line })
  })
  return found
}

/** Binary files hold a NUL byte early on (the heuristic git itself uses). */
export function isBinary(bytes: Uint8Array): boolean {
  return bytes.subarray(0, 8000).includes(0)
}

async function trackedFiles(root: string): Promise<string[]> {
  const out = await Bun.$`git ls-files -z`.cwd(root).quiet().text()
  return out.split('\0').filter(Boolean)
}

async function main(): Promise<void> {
  const root = resolve(import.meta.dir, '..')
  const problems: string[] = []
  const files = await trackedFiles(root)
  for (const rel of files) {
    const file = Bun.file(resolve(root, rel))
    if (!(await file.exists())) continue // deleted in the working tree, not yet committed
    const bytes = new Uint8Array(await file.arrayBuffer())
    if (isBinary(bytes)) continue
    for (const m of findConflictMarkers(new TextDecoder().decode(bytes))) problems.push(`${rel}:${m.line}: ${m.text.slice(0, 60)}`)
  }
  if (problems.length) {
    console.error(`✗ Conflict markers left in tracked files (${problems.length}):\n`)
    for (const p of problems) console.error('  ' + p)
    process.exit(1)
  }
  console.log(`✓ No conflict markers (${files.length} tracked files)`)
}

if (import.meta.main) await main()
