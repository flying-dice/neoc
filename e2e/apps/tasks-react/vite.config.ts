import react from '@vitejs/plugin-react'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// `vos()` reads openrpc.json, generates the typed client and the Luau router into .app/, and builds the app ref into dist/.
export default defineConfig({
  plugins: [react(), vos({ client: 'src/main.tsx' })]
})
