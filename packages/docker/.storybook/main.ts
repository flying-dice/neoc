import { defineStorybook } from '@vos/storybook/main'

/** The container framework's Storybook: the parts every app that runs on Docker shows the same way. */
export default defineStorybook({
  stories: ['../src/**/*.stories.svelte'],
  viteConfigPath: '.storybook/vite.config.ts'
})
