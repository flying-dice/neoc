import { defineStorybook } from '@agentos/storybook/main'

/** The desktop's own Storybook: the shell and every built-in app. */
export default defineStorybook({
  stories: ['../src/renderer/src/**/*.stories.svelte'],
  viteConfigPath: '.storybook/vite.config.ts'
})
