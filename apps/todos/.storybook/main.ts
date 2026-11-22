import { defineStorybook } from '@vos/storybook/main'

/** The Todos app's own Storybook: its page in a window, and the parts the page is made of. */
export default defineStorybook({
  stories: ['../src/**/*.stories.svelte'],
  viteConfigPath: '.storybook/vite.config.ts'
})
