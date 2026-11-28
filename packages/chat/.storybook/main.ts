import { defineStorybook } from '@vos/storybook/main'

/** The chat's Storybook: the parts of a chat with an agent, as every app that talks to one shows them. */
export default defineStorybook({
  stories: ['../src/**/*.stories.svelte'],
  viteConfigPath: '.storybook/vite.config.ts'
})
