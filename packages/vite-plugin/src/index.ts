/**
 * The vos Vite plugin. An app is one Vite project: its client in any framework, its backend beside it (Luau, or TypeScript
 * that the build compiles to Luau: `typescript-backend.ts`), and one contract (`openrpc.json`: OpenRPC, JSON-RPC 2.0) that
 * both sides are generated from.
 *
 *     // vite.config.ts
 *     import { vos } from '@vos/vite-plugin'
 *     export default defineConfig({ plugins: [svelte(), vos()] })
 *
 * The client is a normal Vite app: an `index.html` at the project root, its scripts, styles and assets, built into
 * `dist/static/` with relative URLs (code splitting works as in any Vite app). There is no `mount(el)`: the page starts
 * itself: `import { api } from '@<app id>/rpc'` is the app's backend, and `import { host } from '@vos/host'` the host page that
 * holds it.
 *
 * What the plugin does:
 *   - generates `.app/` from the contract when the build starts (and whenever the contract changes, in watch mode):
 *     `rpc.d.ts` and `rpc.js` (the `@<app id>/rpc` module), `host.d.ts` and `host.js` (the `@vos/host` module), `tracing.d.ts` and `tracing.js` (the `@vos/tracing` module: spans
 *     and events, after Rust's `tracing`), `rpc.luau` (the types, the `Handlers` interface the developer
 *     implements, the router) and `entry.luau` (the backend's entry: the handlers behind the router);
 *   - fails the build when `server/main.luau` does not implement the generated `Handlers` interface (a declared method with
 *     no handler, or one of the wrong shape): luau-lsp's strict check of the backend, so valid Luau of any shape passes;
 *   - puts the host's tags first in `index.html`'s head at build time: the kit's stylesheet and script (`/kit/tokens.css`,
 *     `/kit/kit.js`) and the bridge (`/apps/bridge.js`), one copy each served by the host, never bundled (`hostTags`);
 *   - writes the kit's editor types for the framework the project depends on (`.app/kit-svelte.d.ts`, `.app/kit-react.d.ts`);
 *   - for a backend in TypeScript (`server/main.ts`), generates `.app/server/` instead of the two Luau files (the host's
 *     declarations, the types and the router of the contract, the entry) and compiles it: a type error fails the build;
 *   - bundles the backend into `dist/app.luau` and writes `dist/app-ref.json`, the app ref the host installs: the
 *     manifest, the backend, and every file of `dist/static/` with its sha256.
 *
 * Everything it generates is a list of `{ path, contents }` (`generate.ts`).
 */
import { existsSync, mkdirSync, mkdtempSync, readdirSync, rmSync, statSync } from 'node:fs'
import { join, resolve } from 'node:path'
import type { HtmlTagDescriptor, Logger, Plugin } from 'vite'
import { backendBundle, promote, writeApp } from './backend.ts'
import { rpcModule } from './emit-client.ts'
import { DEFAULTS, TYPESCRIPT_BACKEND, generate, isTypeScript, readDefinition, removeOtherBackend, resolveOptions, write, type Options } from './generate.ts'
import { analyze, ToolMissing } from './luau-tools.ts'

export type { Options } from './generate.ts'
export { DefinitionError, parseDefinition } from './definition.ts'

/** Where the built client goes, under the build's output folder: the app ref lists it as the app's static content. */
export const STATIC_DIR = 'static'

/** The host's bridge script: the host serves one copy to every app's page. */
const BRIDGE = '/apps/bridge.js'
/** The kit (NEOC-252): its tokens and base styles, and its web components. The host serves one copy of each to every app. */
const KIT_CSS = '/kit/tokens.css'
const KIT_JS = '/kit/kit.js'

/**
 * The tags the plugin puts first in every app's head, in this order: the kit's stylesheet (before first paint, and before the
 * app's own styles, which then win over the kit's base), the kit's script (a classic script, so it runs before the app's
 * module and the kit's elements are defined when the app first renders), then the host's bridge. The stylesheet is
 * loaded `crossorigin`, as Vite loads the app's own: the page has an opaque origin, and the host lets it read `/kit/` so
 * the page (and a checker such as axe running in it) can read the rules. Every app gets them, whether it uses the kit or not.
 */
export function hostTags(): HtmlTagDescriptor[] {
  return [
    { tag: 'link', attrs: { rel: 'stylesheet', href: KIT_CSS, crossorigin: '' }, injectTo: 'head-prepend' },
    { tag: 'script', attrs: { src: KIT_JS }, injectTo: 'head-prepend' },
    { tag: 'script', attrs: { src: BRIDGE }, injectTo: 'head-prepend' }
  ]
}

/**
 * The page without any tag of its own that loads what the plugin injects (the kit's two files, the bridge): an app that
 * added them by hand gets them once. `removed` names what was taken out, for the build's message.
 */
export function withoutHostTags(html: string): { page: string; removed: string[] } {
  const removed: string[] = []
  let page = html
  for (const url of [KIT_CSS, KIT_JS, BRIDGE]) {
    const escaped = url.replace(/[.*+?^${}()|[\]\\/]/g, '\\$&')
    const tag = new RegExp(`[ \\t]*<(?:link|script)\\b[^>]*\\b(?:href|src)=["']${escaped}["'][^>]*>(?:\\s*</script>)?[ \\t]*\\r?\\n?`, 'gi')
    if (tag.test(page)) {
      removed.push(url)
      page = page.replace(tag, '')
    }
  }
  return { page, removed }
}

/** Every `.luau` and `.ts` file under `dir` (the backend's modules: the build bundles them, so a change to one rebuilds). */
function sourcesUnder(dir: string): string[] {
  if (!existsSync(dir)) return []
  return readdirSync(dir).flatMap((name) => {
    const path = join(dir, name)
    return statSync(path).isDirectory() ? sourcesUnder(path) : name.endsWith('.luau') || name.endsWith('.ts') ? [path] : []
  })
}

export function vos(options: Partial<Options> = {}): Plugin {
  // the backend's language is read from the project once its root is known (`config`)
  let settings: Options = { ...DEFAULTS, ...options }
  let root = process.cwd()
  let distDir = resolve(process.cwd(), 'dist')
  /**
   * Where this build is made: a folder of its own (a unique name from the platform's own routine for temporary directories, inside
   * the project so the move into `dist/` stays on one file system), so two builds at once (the watcher and a one-off build) never
   * share it. `dist/` is only touched once every step has succeeded.
   */
  let staging = resolve(process.cwd(), settings.generated)
  // a build that failed must not write an app ref: the one on disk (if any) is the last build that worked
  let failed = false
  let logger: Logger | undefined
  /** A TypeScript backend as this build compiled it when it started (the compiler runs once per build). */
  let compiled: string | undefined
  /** The name this app's typed client is imported by (`@<app id>/rpc`), once the contract has been read. */
  let ownRpc: string | undefined

  /**
   * Generate, and check the backend against the contract. The check is a type checker's (luau-lsp for a backend in Luau,
   * the TypeScript compiler for one in TypeScript, whose compiled bundle is kept in `compiled`), not a pattern over the
   * source: the generated `Handlers` type has one function per declared method, so a missing handler, a handler of the
   * wrong shape, or one that is not declared is a type error in the backend, and valid code of any shape (a handler
   * defined in another module, assigned from a local function, added to the table later) never fails.
   */
  const regenerate = (): void => {
    write(root, generate(root, settings))
    removeOtherBackend(root, settings)
    const backend = resolve(root, settings.backend)
    if (!existsSync(backend)) {
      const either = options.backend === undefined ? ` (or ${TYPESCRIPT_BACKEND}, for a backend in TypeScript)` : ''
      throw new Error(`${settings.backend}${either} not found: the backend returns one handler per method of ${settings.definition}`)
    }
    // a backend in TypeScript is checked by compiling it: the generated entry passes its handlers to the router, whose
    // parameter is the `Handlers` of the definition. What it compiles to is kept for the end of the build.
    if (isTypeScript(settings)) {
      compiled = backendBundle(root, settings)
      return
    }
    if (!settings.check) return
    let result: ReturnType<typeof analyze>
    try {
      result = analyze(root, 'throw')
    } catch (error) {
      // a build that cannot check the backend cannot say the app will load, so it stops, as it does without darklua
      if (error instanceof ToolMissing) {
        throw new Error(`${error.message}\n  (to build without the check: vos({ check: false }), and the host refuses an app whose handlers do not match its contract when it loads it)`)
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
    configResolved(config) {
      logger = config.logger
    },
    config(config) {
      root = resolve(config.root ?? process.cwd())
      settings = resolveOptions(root, options)
      distDir = resolve(root, config.build?.outDir ?? 'dist')
      mkdirSync(resolve(root, settings.generated), { recursive: true })
      staging = mkdtempSync(join(resolve(root, settings.generated), 'build-'))
      // whatever happens to the build, the folder does not outlive the process
      process.once('exit', () => rmSync(staging, { recursive: true, force: true }))
      return {
        // relative URLs: the page is served at `/apps/<id>/static/`, wherever the host mounts it
        base: './',
        build: {
          // built next to `dist/`, in the generated folder: a build that fails midway leaves `dist/` as it was
          outDir: join(staging, STATIC_DIR),
          emptyOutDir: true,
          // the browsers vos runs on have `modulepreload`: no polyfill to ship in every app
          modulePreload: { polyfill: false }
        }
      }
    },
    transformIndexHtml: {
      // after Vite's own pass over the page, so it does not try to bundle (and warn about) the host's classic scripts
      order: 'post',
      handler(html) {
        const { page, removed } = withoutHostTags(html)
        if (removed.length > 0)
          logger?.warn(`vos: index.html names ${removed.join(' and ')} itself; the plugin puts the host's tags into every page, so remove yours (it was left out, not loaded twice)`)
        return { html: page, tags: hostTags() }
      }
    },
    resolveId(id) {
      // the app's own typed client: `@<its id>/rpc`, read from the contract once
      ownRpc ??= rpcModule(readDefinition(root, settings).app.id)
      if (id === ownRpc) return resolve(root, settings.generated, 'rpc.js')
      if (id === '@vos/host') return resolve(root, settings.generated, 'host.js')
      return id === '@vos/tracing' ? resolve(root, settings.generated, 'tracing.js') : null
    },
    buildStart() {
      failed = false
      compiled = undefined
      rmSync(staging, { recursive: true, force: true })
      // a build that does not check the backend says so, every time: nothing else will
      if (!settings.check && !isTypeScript(settings)) this.warn(`vos: the check is off (check: false): nothing verifies that ${settings.backend} implements every method of ${settings.definition}`)
      if (options.backend === undefined && !isTypeScript(settings) && existsSync(resolve(root, TYPESCRIPT_BACKEND)))
        this.warn(`vos: ${settings.backend} and ${TYPESCRIPT_BACKEND} are both there: the backend is ${settings.backend}, and ${TYPESCRIPT_BACKEND} is not built or checked`)
      // watched before anything can fail: a watching build whose first run stops on an error is rebuilt by the fix
      this.addWatchFile(resolve(root, settings.definition))
      for (const file of sourcesUnder(resolve(root, settings.backend, '..'))) this.addWatchFile(file)
      regenerate()
    },
    buildEnd(error) {
      if (error) failed = true
    },
    closeBundle() {
      try {
        const staticDir = join(staging, STATIC_DIR)
        // a build that failed has no page: its own error is the one to read
        if (failed || !existsSync(join(staticDir, 'index.html'))) return
        writeApp(root, staging, readDefinition(root, settings), compiled ?? backendBundle(root, settings), STATIC_DIR)
        promote(staging, distDir)
      } finally {
        // whatever happened, nothing of this build stays behind in the generated folder
        rmSync(staging, { recursive: true, force: true })
      }
    }
  }
}
