import type { Preview } from '@storybook/web-components-vite'
import { withThemeByDataAttribute } from '@storybook/addon-themes'
import '../../ui/src/styles/fonts.css'
import 'virtual:kit-tokens.css'
import '../src/index'
import './preview.css'

const preview: Preview = {
  parameters: {
    layout: 'centered',
    controls: { expanded: true },
    backgrounds: { disable: true },
    a11y: { test: 'error' },
    options: { storySort: { order: ['Introduction', 'Controls', 'Lists', 'Layout', 'Feedback'] } }
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
