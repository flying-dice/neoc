import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Used by Storybook. The desktop app consumes the kit's sources through its own Vite config.
export default defineConfig({
  plugins: [svelte()]
})
