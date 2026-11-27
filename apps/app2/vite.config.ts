import { svelte } from '@sveltejs/vite-plugin-svelte'
import { vos } from '@vos/vite-plugin'
import { defineConfig } from 'vite'

// App 2 depends on App 1: the build adds App 1's types (its public requests and notifications) for the page and the backend.
export default defineConfig({
  plugins: [svelte({ configFile: false }), vos({ dependencies: ['../app1'] })]
})
