/**
 * Finding a command-line tool (darklua, luau-lsp, stylua) the way a shell would, on every platform: the folders of `PATH`
 * in order, then the extra places given. There is no `which` to ask on Windows, and a program there has an extension
 * (`darklua.exe`), so the search is done here.
 */
import { accessSync, constants, statSync } from 'node:fs'
import { delimiter, join } from 'node:path'

/** The endings a program's file may have: none, and on Windows those of `PATHEXT` (`.EXE`, `.CMD`, ...). */
function endings(): string[] {
  if (process.platform !== 'win32') return ['']
  return ['', ...(process.env.PATHEXT ?? '.COM;.EXE;.BAT;.CMD').split(';').filter(Boolean)]
}

function isProgram(file: string): boolean {
  try {
    if (!statSync(file).isFile()) return false
    accessSync(file, constants.X_OK)
    return true
  } catch {
    return false
  }
}

/** The program at `path`, with one of the platform's endings when it has none of its own. */
function programAt(path: string): string | undefined {
  for (const ending of endings()) {
    if (isProgram(path + ending)) return path + ending
  }
  return undefined
}

/** The full path of the tool `name`: the first on `PATH`, else the first of `extra` (paths without an ending) that exists. */
export function findTool(name: string, ...extra: string[]): string | undefined {
  const folders = (process.env.PATH ?? '').split(delimiter).filter(Boolean)
  for (const candidate of [...folders.map((folder) => join(folder, name)), ...extra]) {
    const found = programAt(candidate)
    if (found) return found
  }
  return undefined
}
