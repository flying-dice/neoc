import type { Preview } from '@storybook/svelte-vite'
import { withThemeByDataAttribute } from '@storybook/addon-themes'
import '../src/styles/index.css'
import './preview.css'

const preview: Preview = {
  parameters: {
    layout: 'centered',
    controls: {
      expanded: true,
      matchers: { color: /(background|color|tint)$/i }
    },
    backgrounds: { disable: true },
    a11y: { test: 'todo' },
    options: {
      storySort: {
        order: ['Introduction', 'Tokens', 'Atoms', 'Molecules', 'Organisms', 'Templates']
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
