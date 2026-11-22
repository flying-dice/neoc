import type { StorybookConfig } from '@storybook/web-components-vite'

const config: StorybookConfig = {
  stories: ['../src/**/*.stories.ts'],
  addons: ['@storybook/addon-docs', '@storybook/addon-a11y', '@storybook/addon-themes'],
  framework: { name: '@storybook/web-components-vite', options: {} },
  core: { disableTelemetry: true },
  viteFinal: (config) => {
    // vite.config.ts also builds /kit/kit.js (a library build); Storybook builds its own app from the same aliases.
    if (config.build) delete config.build.lib
    return config
  }
}

export default config
