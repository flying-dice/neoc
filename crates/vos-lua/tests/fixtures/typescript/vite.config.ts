import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// `vos()` reads package.json and openrpc.json, writes the types into types/, compiles the backend (server/main.ts) to
// Luau and builds the app ref into dist/ (`dist/app.luau` is what the tests load).
export default defineConfig({ plugins: [vos()] })
