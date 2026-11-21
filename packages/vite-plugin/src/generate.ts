/**
 * Generation: the definition in, the typed client and the Luau router out. The output is committed with the app (the
 * editor and the linters see it without a build), and `vos generate --check` fails when it is stale.
 */
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs'
import { join } from 'node:path'
import { parseDefinition, type Definition } from './definition.ts'
import { emitLuau } from './emit-luau.ts'
import { emitTs } from './emit-ts.ts'

export interface Options {
  /** The definition, relative to the project (default `rpc.openrpc.json`). */
  definition: string
  /** Where the generated files go, relative to the project (default `generated`). */
  generated: string
  /** The client's entry module (default `src/main.ts`). */
  client: string
  /** The backend's entry (default `app.luau`). */
  backend: string
}

export const DEFAULTS: Options = {
  definition: 'rpc.openrpc.json',
  generated: 'generated',
  client: 'src/main.ts',
  backend: 'app.luau'
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

/** The files the definition generates. */
export function generate(project: string, options: Options): GeneratedFile[] {
  const definition = readDefinition(project, options)
  return [
    { path: `${options.generated}/rpc.ts`, contents: emitTs(definition, options.definition) },
    { path: `${options.generated}/rpc.luau`, contents: emitLuau(definition, options.definition) }
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

/** The generated files that are missing or differ from what the definition generates now. */
export function stale(project: string, files: GeneratedFile[]): string[] {
  return files
    .filter((f) => !existsSync(join(project, f.path)) || readFileSync(join(project, f.path), 'utf8') !== f.contents)
    .map((f) => f.path)
}
