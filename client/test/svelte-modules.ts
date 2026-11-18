/**
 * `bun test` preload: compiles rune modules (`*.svelte.ts`) so the shell's state classes (`lib/host.svelte.ts`,
 * `windowManager.svelte.ts`, ...) run under plain bun tests. Components (`*.svelte`) are not loaded here: logic that
 * needs one belongs in a Storybook play function.
 */
import { plugin } from 'bun'
import { compileModule } from 'svelte/compiler'

const ts = new Bun.Transpiler({ loader: 'ts' })

plugin({
  name: 'svelte-rune-modules',
  setup(build) {
    build.onLoad({ filter: /\.svelte\.ts$/ }, async ({ path }) => {
      const js = ts.transformSync(await Bun.file(path).text())
      const { js: out } = compileModule(js, { filename: path, generate: 'client' })
      return { contents: out.code, loader: 'js' }
    })
  }
})
