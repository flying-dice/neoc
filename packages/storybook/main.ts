/**
 * Storybook config factory shared by every AgentOS Storybook.
 *
 *   // apps/<app>/.storybook/main.ts
 *   import { defineStorybook } from '@vos/storybook/main'
 *   export default defineStorybook()
 *
 * Stories live next to the code as `*.stories.svelte` (Svelte CSF).
 */
import type { StorybookConfig } from '@storybook/svelte-vite'

export interface StorybookOptions {
  /** Story globs, relative to the `.storybook` folder. */
  stories?: string[]
  /** Other Storybooks to compose into the sidebar (name → URL). */
  refs?: Record<string, { title: string; url: string }>
  /** Vite config for the stories, relative to the package (default: its vite.config.ts). */
  viteConfigPath?: string
}

export function defineStorybook(options: StorybookOptions = {}): StorybookConfig {
  return {
    stories: options.stories ?? ['../docs/**/*.mdx', '../src/**/*.stories.@(svelte|ts)'],
    addons: [
      '@storybook/addon-svelte-csf',
      '@storybook/addon-docs',
      '@storybook/addon-a11y',
      '@storybook/addon-themes'
    ],
    framework: { name: '@storybook/svelte-vite', options: {} },
    core: {
      disableTelemetry: true,
      ...(options.viteConfigPath
        ? { builder: { name: '@storybook/builder-vite', options: { viteConfigPath: options.viteConfigPath } } }
        : {})
    },
    ...(options.refs ? { refs: options.refs } : {})
  }
}
