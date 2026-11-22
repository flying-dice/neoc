/**
 * The kit's build (`bun run build:kit`): Lit and every component in one classic IIFE script, `kit.js`, and the token
 * sheet `tokens.css`, both written to `client/public/kit/`. Vite copies that folder into `client/dist/kit/`, and the host
 * serves the client's folder, so a frame loads them from `/kit/tokens.css` and `/kit/kit.js`. They are generated, not
 * committed. The same config drives the kit's Storybook.
 *
 * A classic script (not a module): an app frame has an opaque origin, and a module script needs CORS, which the host's
 * Origin guard refuses for `Origin: null`. No external URL, no `eval`: the frame's CSP is `script-src 'self'`.
 */
import { resolve } from 'node:path'
import { defineConfig, type Plugin } from 'vite'
import { buildTokensCss } from '../../scripts/kit-tokens.ts'

const here = import.meta.dirname
const ui = resolve(here, '../ui/src')

const TOKENS_ID = 'virtual:kit-tokens.css'

/** The token sheet as a virtual module (Storybook imports it) and as an emitted asset (the build). */
function tokens(): Plugin {
  return {
    name: 'vos-kit-tokens',
    resolveId: (id) => (id === TOKENS_ID ? `\0${TOKENS_ID}` : undefined),
    load: (id) => (id === `\0${TOKENS_ID}` ? buildTokensCss() : undefined),
    generateBundle() {
      this.emitFile({ type: 'asset', fileName: 'tokens.css', source: buildTokensCss() })
    }
  }
}

export default defineConfig({
  root: here,
  publicDir: false,
  plugins: [tokens()],
  resolve: {
    // Pure modules of the shell's kit, reused rather than copied.
    alias: {
      'kit:icons': resolve(ui, 'icons.ts'),
      'kit:contrast': resolve(ui, 'contrast.ts'),
      'kit:keys': resolve(ui, 'keys.ts')
    }
  },
  define: { 'process.env.NODE_ENV': '"production"' },
  build: {
    target: 'es2022',
    outDir: resolve(here, '../../client/public/kit'),
    emptyOutDir: false,
    cssCodeSplit: false,
    lib: { entry: resolve(here, 'src/index.ts'), name: 'VosKit', formats: ['iife'], fileName: () => 'kit.js' }
  }
})
