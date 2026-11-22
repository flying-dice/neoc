/**
 * Generation: the contract in, the typed client and the Luau router out, into the project's `.app/` folder. The output
 * is not committed (`.app/` is ignored): every script of the project (`build`, `typecheck`, `lint`) generates first, and
 * `bun run generate` does it on its own for the editor.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join, relative } from 'node:path'
import { parseDefinition, type Definition } from './definition.ts'
import { emitDeclarations, emitEntry, emitModule } from './emit-client.ts'
import { emitLuau, emitLuauEntry } from './emit-luau.ts'

export interface Options {
  /** The contract, relative to the project (default `openrpc.json`). */
  definition: string
  /** Where the generated files go, relative to the project (default `.app`). */
  generated: string
  /** The client's module: exports `mount(el)` (default `src/main.ts`). */
  client: string
  /** The backend's module: returns the handlers (default `server/main.luau`). */
  backend: string
}

export const DEFAULTS: Options = {
  definition: 'openrpc.json',
  generated: '.app',
  client: 'src/main.ts',
  backend: 'server/main.luau'
}

export interface GeneratedFile {
  /** Relative to the project. */
  path: string
  contents: string
}

export function readDefinition(project: string, options: Options): Definition {
  const file = join(project, options.definition)
  if (!existsSync(file)) throw new Error(`${options.definition} not found in ${project}: an app defines its RPC there (OpenRPC)`)
  return parseDefinition(readFileSync(file, 'utf8'), options.definition)
}

/**
 * What the build checks of the backend, without running it: a declared method with no handler in `server/main.luau` is
 * an error (the app would not load), and a handler the contract does not declare is a warning (the router refuses to
 * load it too, so it is an error at load; the warning says so first).
 */
export function handlerProblems(definition: Definition, backendSource: string): { missing: string[]; undeclared: string[] } {
  const found = new Set<string>()
  for (const m of backendSource.matchAll(/^\s*([A-Za-z_]\w*)\s*=\s*function\b/gm)) found.add(m[1] as string)
  for (const m of backendSource.matchAll(/\bfunction\s+[A-Za-z_][\w]*[.:]([A-Za-z_]\w*)\s*\(/g)) found.add(m[1] as string)
  const declared = new Set(definition.methods.map((m) => m.name))
  return {
    missing: [...declared].filter((name) => !found.has(name)),
    // `run` is the function field of an `app.tool { ... }`, not a handler
    undeclared: [...found].filter((name) => !declared.has(name) && name !== 'run')
  }
}

/** The files the contract generates. */
export function generate(project: string, options: Options): GeneratedFile[] {
  const definition = readDefinition(project, options)
  const dir = options.generated
  const clientFromGenerated = relative(join(project, dir), join(project, options.client)).split('\\').join('/')
  return [
    { path: `${dir}/rpc.d.ts`, contents: emitDeclarations(definition, options.definition) },
    { path: `${dir}/rpc.js`, contents: emitModule(definition, options.definition) },
    { path: `${dir}/entry.js`, contents: emitEntry(clientFromGenerated) },
    { path: `${dir}/rpc.luau`, contents: emitLuau(definition, options.definition) },
    { path: `${dir}/entry.luau`, contents: emitLuauEntry() }
  ]
}

/** Write what changed (a file is never rewritten with the same text, so a watcher is not woken for nothing). Returns the paths written. */
export function write(project: string, files: GeneratedFile[]): string[] {
  const written: string[] = []
  for (const file of files) {
    const target = join(project, file.path)
    if (existsSync(target) && readFileSync(target, 'utf8') === file.contents) continue
    mkdirSync(join(target, '..'), { recursive: true })
    writeFileSync(target, file.contents)
    written.push(file.path)
  }
  return written
}
