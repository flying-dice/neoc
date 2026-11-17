import { defineConfig } from 'vite'
import { svelte } from '@sveltejs/vite-plugin-svelte'

// Used by Storybook. The desktop bundles the app's sources through its own config.
export default defineConfig({ plugins: [svelte()] })
