import { afterEach, describe, expect, test } from 'bun:test'
import { chmodSync, mkdirSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs'
import { tmpdir } from 'node:os'
import { delimiter, join } from 'node:path'
import { findTool } from './find-tool.ts'

const windows = process.platform === 'win32'
const PATH = process.env.PATH
const dirs: string[] = []

/** A folder holding a program called `name` (with the platform's ending); returns the folder and the program's path. */
function folderWith(name: string): { dir: string; program: string } {
  const dir = mkdtempSync(join(tmpdir(), 'vos-find-tool-'))
  dirs.push(dir)
  const program = join(dir, windows ? `${name}.exe` : name)
  writeFileSync(program, '')
  chmodSync(program, 0o755)
  return { dir, program }
}

/** A path as it is compared: Windows file names ignore case, and the ending found there is `PATHEXT`'s (`.EXE`). */
const named = (path: string | undefined): string | undefined => (windows ? path?.toLowerCase() : path)
const found = (name: string, ...extra: string[]): string | undefined => named(findTool(name, ...extra))

afterEach(() => {
  process.env.PATH = PATH
  for (const dir of dirs.splice(0)) rmSync(dir, { recursive: true, force: true })
})

describe('finding a tool', () => {
  test('the first folder of PATH that holds it wins, whatever ending a program has here', () => {
    const first = folderWith('sometool')
    const second = folderWith('sometool')
    process.env.PATH = [first.dir, second.dir].join(delimiter)
    expect(found('sometool')).toBe(named(first.program))
  })

  test('a tool that is not on PATH is found at an extra place, and PATH comes first', () => {
    const extra = folderWith('sometool')
    process.env.PATH = ''
    expect(found('sometool', join(extra.dir, 'sometool'))).toBe(named(extra.program))
    const onPath = folderWith('sometool')
    process.env.PATH = onPath.dir
    expect(found('sometool', join(extra.dir, 'sometool'))).toBe(named(onPath.program))
  })

  test('a tool that is nowhere, and a folder of its name, are not found', () => {
    const { dir } = folderWith('other')
    mkdirSync(join(dir, 'sometool'))
    process.env.PATH = dir
    expect(findTool('sometool')).toBeUndefined()
    expect(findTool('sometool', join(dir, 'absent'))).toBeUndefined()
  })
})
