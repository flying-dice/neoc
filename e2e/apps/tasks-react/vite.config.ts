/** Builds the client into one classic script, `dist/client.js`, that defines `vosApp.mount` (see apps/README.md). */
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

export default defineConfig({
  plugins: [react()],
  // a library build leaves `process.env.NODE_ENV` for the page to define, and a frame has no `process`
  define: { 'process.env.NODE_ENV': '"production"' },
  build: {
    lib: { entry: 'src/main.jsx', name: 'vosApp', formats: ['iife'], fileName: () => 'client.js' },
    outDir: 'dist',
    emptyOutDir: true
  }
})
