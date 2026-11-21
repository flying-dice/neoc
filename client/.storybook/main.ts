import { defineStorybook } from '@vos/storybook/main'

/** The client's own Storybook: the shell and every built-in app. */
export default defineStorybook({
  stories: ['../src/**/*.stories.svelte'],
  viteConfigPath: '.storybook/vite.config.ts'
})
