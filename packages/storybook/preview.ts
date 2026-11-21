/**
 * Shared Storybook preview: AgentOS tokens and base styles, light/dark theme
 * switch (the `theme` global: Light | Dark), and a11y checks set to fail.
 *
 *   // apps/<app>/.storybook/preview.ts
 *   export { default } from '@vos/storybook/preview'
 */
import type { Preview } from '@storybook/svelte-vite'
import { withThemeByDataAttribute } from '@storybook/addon-themes'
import '@vos/ui/styles.css'
import './preview.css'

const preview: Preview = {
  parameters: {
    layout: 'centered',
    controls: {
      expanded: true,
      matchers: { color: /(background|color|tint)$/i }
    },
    backgrounds: { disable: true },
    a11y: { test: 'error' },
    options: {
      storySort: {
        order: ['Introduction', 'Tokens', 'Apps', 'Pages', 'Organisms', 'Molecules', 'Atoms', 'Templates']
      }
    }
  },
  decorators: [
    withThemeByDataAttribute({
      themes: { Light: 'light', Dark: 'dark' },
      defaultTheme: 'Light',
      attributeName: 'data-theme'
    })
  ]
}

export default preview
