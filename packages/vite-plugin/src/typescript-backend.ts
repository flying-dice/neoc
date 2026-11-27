/**
 * A backend written in TypeScript: `server/main.ts` beside the page, compiled to Luau by TypeScriptToLua as part of the
 * same build. The developer writes what a Luau backend does, against the globals the host gives it:
 *
 *     import { db } from 'vos:app'
 *     RPC.on_request('add', (params, ctx) => ...)
 *
 * `types/vos.backend.d.ts` (`emit-backend-ts.ts`) types those globals for the app, and `types/tsconfig.backend.json` says how
 * the backend is checked: by the build, which compiles with it, and by the editor, through the `server/tsconfig.json`
 * that extends it. A type error is a failed build; a wrong method name, params or result in `RPC` is one.
 *
 * The compiler's output is one Luau chunk, the app's `app.luau`: TypeScriptToLua bundles the modules itself, so darklua,
 * luau-lsp and StyLua have no part in a TypeScript backend.
 *
 * What is ours and not TypeScriptToLua's (1.34 has a `Luau` target that writes Luau's `continue` and `if` expressions and
 * is otherwise Lua 5.4):
 *   - the bitwise operators are written as calls of Luau's `bit32` (5.4's `&`, `|`, `~`, `<<`, `>>` are not Luau), and a
 *     compound one (`x &= y`) is refused with a message. The results are `bit32`'s, not JavaScript's: unsigned (`-8 >> 1`
 *     is 4294967292, `~0` is 4294967295), and a shift by 32 or more gives 0 (or all ones, for `>>` of a negative number)
 *     where JavaScript shifts by the count modulo 32;
 *   - `Date`, `JSON`, `Intl`, `Reflect` and `Proxy` are refused with a message: TypeScript's library declares them and
 *     nothing in an app's environment is them, so they would compile and fail when called;
 *   - its `Error` reads the stack through the `debug` library, which an app's environment does not have (and Luau's has no
 *     `getinfo`): the bundle starts with a local `debug` that answers with no stack, so `throw new Error("...")` and
 *     `String(error)` work.
 */
import { readFileSync } from 'node:fs'
import { createRequire } from 'node:module'
import { basename, dirname, join, relative, resolve } from 'node:path'
import ts from 'typescript'
import * as tstl from 'typescript-to-lua'

/** The backend's entry, relative to the project. */
export const BACKEND_TS = 'server/main.ts'
/** What the build writes for a TypeScript backend, under the generated folder: its declarations and how it is checked. */
export const BACKEND_DECLARATIONS = 'vos.backend.d.ts'
export const BACKEND_TSCONFIG = 'tsconfig.backend.json'

/** The chunk the compiler writes: the whole backend. Never on disk: the build takes it from memory. */
const BUNDLE = 'app.lua'

/** A path as TypeScript and JSON write it: `/` between folders on every platform. */
export const posix = (path: string): string => path.replaceAll('\\', '/')

/**
 * The folder TypeScript finds `@typescript-to-lua/language-extensions` in (the types of what the compiler adds to the
 * language: `LuaMultiReturn`, `LuaTable`), which the host's declarations reference: the plugin's own copy, so a project
 * needs nothing installed for it.
 */
export function typeRoot(): string {
  return posix(resolve(dirname(createRequire(import.meta.url).resolve('@typescript-to-lua/language-extensions/package.json')), '../..'))
}

/** The modules the declarations declare (`declare module "std:fs"`): the host answers them, so the compiler leaves their `require` alone. */
export function declaredModules(declarations: string): string[] {
  return [...declarations.matchAll(/^declare module "([^"]+)" \{$/gm)].map((m) => m[1] as string)
}

/** What the bundle starts with: see the module's doc. */
const PROLOGUE = `-- An app's environment has no \`debug\` library: TypeScriptToLua's Error asks it for a stack, and gets none.
local debug = { getinfo = function() return nil end, traceback = function() return "" end }
`

const BIT32: Partial<Record<ts.SyntaxKind, string>> = {
  [ts.SyntaxKind.AmpersandToken]: 'band',
  [ts.SyntaxKind.BarToken]: 'bor',
  [ts.SyntaxKind.CaretToken]: 'bxor',
  [ts.SyntaxKind.LessThanLessThanToken]: 'lshift',
  [ts.SyntaxKind.GreaterThanGreaterThanToken]: 'arshift',
  [ts.SyntaxKind.GreaterThanGreaterThanGreaterThanToken]: 'rshift'
}

const COMPOUND_BITWISE = new Set<ts.SyntaxKind>([
  ts.SyntaxKind.AmpersandEqualsToken,
  ts.SyntaxKind.BarEqualsToken,
  ts.SyntaxKind.CaretEqualsToken,
  ts.SyntaxKind.LessThanLessThanEqualsToken,
  ts.SyntaxKind.GreaterThanGreaterThanEqualsToken,
  ts.SyntaxKind.GreaterThanGreaterThanGreaterThanEqualsToken
])

const bit32 = (name: string, args: tstl.Expression[], node: ts.Node): tstl.Expression =>
  tstl.createCallExpression(tstl.createTableIndexExpression(tstl.createIdentifier('bit32'), tstl.createStringLiteral(name)), args, node)

/** The compound bitwise assignments of a file, each as an error: the compiler would write them with Lua 5.4's operators. */
function compoundBitwise(file: ts.SourceFile): ts.Diagnostic[] {
  const found: ts.Diagnostic[] = []
  const visit = (node: ts.Node): void => {
    if (ts.isBinaryExpression(node) && COMPOUND_BITWISE.has(node.operatorToken.kind)) {
      const operator = node.operatorToken.getText(file)
      found.push({
        file,
        start: node.operatorToken.getStart(file),
        length: node.operatorToken.getWidth(file),
        category: ts.DiagnosticCategory.Error,
        code: 0,
        source: 'vos',
        messageText: `\`${operator}\` is not compiled for Luau: write the assignment out (\`x = x ${operator.slice(0, -1)} y\`)`
      })
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return found
}

/** What TypeScript's library declares and an app's environment does not have, with what to use in its place. */
const NOT_THERE: Record<string, string> = {
  Date: 'use `std:time` or `lib:chrono`',
  JSON: 'use `lib:json`',
  Intl: 'use the host\'s modules (`lib:text`, `lib:unicode`)',
  Reflect: 'there is no reflection in an app',
  Proxy: 'there are no proxies in an app'
}

/**
 * The uses of a global of [`NOT_THERE`] in a file, each as an error: they would compile, and be nil when the app runs.
 * It catches the name written as a name, not every way to reach it (`globalThis["Date"]` is not seen).
 */
function globalsNotThere(program: ts.Program, file: ts.SourceFile): ts.Diagnostic[] {
  const checker = program.getTypeChecker()
  const found: ts.Diagnostic[] = []
  const visit = (node: ts.Node): void => {
    if (ts.isIdentifier(node) && Object.hasOwn(NOT_THERE, node.text)) {
      // `{ Date }` names the property and the value at once: the value is the one that matters
      const symbol = ts.isShorthandPropertyAssignment(node.parent) ? checker.getShorthandAssignmentValueSymbol(node.parent) : checker.getSymbolAtLocation(node)
      const declarations = symbol?.declarations ?? []
      // the library's own, not something of the same name the developer wrote
      if (declarations.length > 0 && declarations.every((d) => program.isSourceFileDefaultLibrary(d.getSourceFile()))) {
        found.push({
          file,
          start: node.getStart(file),
          length: node.getWidth(file),
          category: ts.DiagnosticCategory.Error,
          code: 0,
          source: 'vos',
          messageText: `\`${node.text}\` is not in an app's environment: ${NOT_THERE[node.text]}`
        })
      }
    }
    ts.forEachChild(node, visit)
  }
  visit(file)
  return found
}

/** What the build adds to TypeScriptToLua for Luau: see the module's doc. */
const luauPlugin: tstl.Plugin = {
  beforeTransform: (program) =>
    program
      .getSourceFiles()
      .filter((file) => !file.isDeclarationFile)
      .flatMap((file) => [...compoundBitwise(file), ...globalsNotThere(program, file)]),
  visitors: {
    [ts.SyntaxKind.BinaryExpression]: (node, context) => {
      const name = BIT32[node.operatorToken.kind]
      if (name === undefined) return context.superTransformExpression(node)
      return bit32(name, [context.transformExpression(node.left), context.transformExpression(node.right)], node)
    },
    [ts.SyntaxKind.PrefixUnaryExpression]: (node, context) =>
      node.operator === ts.SyntaxKind.TildeToken ? bit32('bnot', [context.transformExpression(node.operand)], node) : context.superTransformExpression(node)
  },
  beforeEmit: (_program, _options, _host, files) => {
    for (const file of files) file.code = PROLOGUE + file.code
  }
}

/** A problem the compiler found, as `file:line:col: message` with the file relative to the project. */
function describe(project: string, diagnostic: ts.Diagnostic): string {
  const message = ts.flattenDiagnosticMessageText(diagnostic.messageText, '\n')
  const code = diagnostic.code === 0 ? '' : ` (${diagnostic.source === 'typescript-to-lua' ? 'TSTL' : 'TS'}${diagnostic.code})`
  if (diagnostic.file === undefined || diagnostic.start === undefined) return `${message}${code}`
  const { line, character } = diagnostic.file.getLineAndCharacterOfPosition(diagnostic.start)
  return `${posix(relative(project, diagnostic.file.fileName))}:${line + 1}:${character + 1}: ${message}${code}`
}

/** The backend did not compile: `problems` are the compiler's, one per entry. */
export class BackendError extends Error {
  readonly problems: string[]
  constructor(backend: string, problems: string[]) {
    const shown = problems.slice(0, 8).join('\n')
    const more = problems.length > 8 ? `\n... and ${problems.length - 8} more` : ''
    super(`${backend} does not compile (TypeScript, to Luau):\n${shown}${more}`)
    this.problems = problems
  }
}

/**
 * The Luau text of the backend: `server/main.ts` and everything it imports, type checked against `types/` and compiled into
 * one chunk. The generated files must be there (`generate`). Throws a [`BackendError`] that names every problem: a type
 * error (an `RPC` call or handler that does not match the contract is one), and what TypeScriptToLua does not compile.
 */
export function compileBackend(project: string, generated: string): string {
  const dir = resolve(project, generated)
  const declarations = readFileSync(join(dir, BACKEND_DECLARATIONS), 'utf8')
  let bundle: string | undefined
  const result = tstl.transpileProject(
    join(dir, BACKEND_TSCONFIG),
    {
      noEmit: false,
      noEmitOnError: true,
      // nothing is written: the one file of the output is taken below
      outDir: join(dir, 'out'),
      luaTarget: tstl.LuaTarget.Luau,
      luaBundle: BUNDLE,
      luaBundleEntry: resolve(project, BACKEND_TS),
      luaLibImport: tstl.LuaLibImportKind.RequireMinimal,
      noHeader: true,
      // a traceback through source maps needs the `debug` library
      sourceMapTraceback: false,
      noResolvePaths: declaredModules(declarations),
      luaPlugins: [{ plugin: luauPlugin }]
    },
    (file, text) => {
      if (basename(file) === BUNDLE) bundle = text
    }
  )
  const problems = result.diagnostics.filter((d) => d.category === ts.DiagnosticCategory.Error).map((d) => describe(project, d))
  if (problems.length > 0) throw new BackendError(BACKEND_TS, problems)
  if (bundle === undefined) throw new BackendError(BACKEND_TS, ['the compiler wrote no bundle'])
  return bundle
}
