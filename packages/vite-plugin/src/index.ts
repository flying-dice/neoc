/**
 * The vos Vite plugin. An app is one Vite project: its client in any framework, its Luau backend beside it, and one
 * contract (`openrpc.json`: OpenRPC, JSON-RPC 2.0) that both sides are generated from.
 *
 *     // vite.config.ts
 *     import { vos } from '@vos/vite-plugin'
 *     export default defineConfig({ plugins: [svelte(), vos()] })
 *
 * What it does:
 *   - generates `.app/` from the contract when the build starts (and whenever the contract changes, in watch mode):
 *     `rpc.d.ts` and `rpc.js` (the `app:rpc` module: `import { api, host } from 'app:rpc'`), `entry.js` (binds `app:rpc` to the
 *     host, then calls the app's `mount(el)`), `rpc.luau` (the types, the `Handlers` interface the developer implements, the
 *     router) and `entry.luau` (the backend's entry: the handlers behind the router);
 *   - fails the build when `server/main.luau` does not implement the generated `Handlers` interface (a declared method with
 *     no handler, or one of the wrong shape): luau-lsp's strict check of the backend, so valid Luau of any shape passes;
 *   - builds the client into one classic script that defines `vosApp.mount` (`dist/client.js`, CSS folded in);
 *   - bundles the Luau backend into `dist/app.luau` and writes `dist/app-ref.json`, the app ref the host installs.
 *
 * Room for more generated files: everything it writes is a list of `{ path, contents }` (`generate.ts`), so a later step that
 * hands an app other definitions (the UI kit's tags for the editor, say) is one more entry in that list.
 */
import { existsSync, readFileSync, readdirSync, rmSync, statSync, writeFileSync } from 'node:fs'
import { join, resolve } from 'node:path'
import type { Plugin } from 'vite'
import { writeApp } from './backend.ts'
import { DEFAULTS, generate, readDefinition, write, type Options } from './generate.ts'
import { analyze, isGateRun, ToolMissing } from './luau-tools.ts'

export type { Options } from './generate.ts'
export { DefinitionError, parseDefinition } from './definition.ts'

const CLIENT = 'client.js'

/** Every `.luau` file under `dir` (the backend's modules: the build bundles them, so a change to one rebuilds). */
function luauUnder(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() ? luauUnder(path) : name.endsWith('.luau') ? [path] : []
  })
}

/** The app is one script: any CSS the bundle emitted is folded into it, added to the frame's document when the script runs. */
function foldCss(outDir: string): void {
  const css = join(outDir, 'style.css')
  if (!existsSync(css)) return
  const script = join(outDir, CLIENT)
  const inject = `(function(){var s=document.createElement('style');s.textContent=${JSON.stringify(readFileSync(css, 'utf8'))};document.head.appendChild(s)})();\n`
  writeFileSync(script, inject + readFileSync(script, 'utf8'))
  rmSync(css)
}

export function vos(options: Partial<Options> = {}): Plugin {
  const settings: Options = { ...DEFAULTS, ...options }
  let root = process.cwd()
  let outDir = 'dist'
  // a build that failed must not write an app ref: the one on disk (if any) is the last build that worked
  let failed = false

  /**
   * Generate, and check the backend against the contract. The check is the Luau type checker's, not a pattern over the
   * source: the generated `Handlers` type has one function per declared method, so a missing handler, a handler of the
   * wrong shape, or one that is not declared is a type error in `server/main.luau`, and valid code of any shape (a handler
   * defined in a required module, assigned from a local function, added to the table later) never fails.
   */
  const regenerate = (warn: (message: string) => void): void => {
    write(root, generate(root, settings))
    const backend = resolve(root, settings.backend)
    if (!existsSync(backend)) throw new Error(`${settings.backend} not found: the backend returns one handler per method of ${settings.definition}`)
    if (!settings.check) return
    let result: ReturnType<typeof analyze>
    try {
      result = analyze(root, 'throw')
    } catch (error) {
      if (error instanceof ToolMissing && !isGateRun()) {
        warn(`${error.message}\n  the backend was not checked against ${settings.definition}; the host refuses to load an app whose handlers do not match its contract`)
        return
      }
      throw error
    }
    if (result.findings.length > 0) {
      const shown = result.findings.slice(0, 8).join('\n')
      const more = result.findings.length > 8 ? `\n... and ${result.findings.length - 8} more` : ''
      throw new Error(
        `${settings.backend} does not match ${settings.definition} (luau-lsp, strict): every declared method needs a handler of the shape the contract gives it.\n${shown}${more}\nRun \`bun run typecheck\` for the whole report.`
      )
    }
  }

  return {
    name: 'vos',
    config(config) {
      root = resolve(config.root ?? process.cwd())
      outDir = config.build?.outDir ?? 'dist'
      return {
        // a classic script, not a module: an app's frame has an opaque origin, where a module script needs CORS
        build: {
          lib: { entry: resolve(root, settings.generated, 'entry.js'), name: 'vosApp', formats: ['iife'], fileName: () => CLIENT },
          outDir,
          emptyOutDir: true,
          cssCodeSplit: false
        },
        // a library build leaves `process.env.NODE_ENV` to the page, and a frame has no `process`
        define: { 'process.env.NODE_ENV': JSON.stringify('production') }
      }
    },
    resolveId(id) {
      return id === 'app:rpc' ? resolve(root, settings.generated, 'rpc.js') : null
    },
    buildStart() {
      failed = false
      regenerate((message) => this.warn(message))
      this.addWatchFile(resolve(root, settings.definition))
      for (const file of luauUnder(resolve(root, settings.backend, '..'))) this.addWatchFile(file)
    },
    buildEnd(error) {
      if (error) failed = true
    },
    closeBundle() {
      if (failed) return
      const out = resolve(root, outDir)
      // a build that failed has no client script: its own error is the one to read
      if (!existsSync(join(out, CLIENT))) return
      foldCss(out)
      writeApp(root, out, readDefinition(root, settings), `${settings.generated}/entry.luau`, CLIENT)
    }
  }
}
