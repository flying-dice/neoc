import type { StorybookConfig } from '@storybook/svelte-vite'

const config: StorybookConfig = {
  stories: ['../docs/**/*.mdx', '../src/**/*.stories.@(svelte|ts)'],
  addons: [
    '@storybook/addon-svelte-csf',
    '@storybook/addon-docs',
    '@storybook/addon-a11y',
    '@storybook/addon-themes'
  ],
  framework: {
    name: '@storybook/svelte-vite',
    options: {}
  },
  core: {
    disableTelemetry: true
  }
}

export default config
