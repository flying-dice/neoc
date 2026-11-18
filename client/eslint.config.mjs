import { defineConfig } from 'eslint/config'
import tseslint from '@electron-toolkit/eslint-config-ts'
import eslintConfigPrettier from '@electron-toolkit/eslint-config-prettier'
import eslintPluginSvelte from 'eslint-plugin-svelte'

export default defineConfig(
  {
    ignores: [
      '**/node_modules',
      '**/dist',
      '**/out',
      '**/storybook-static',
      '**/test-results',
      '**/playwright-report'
    ]
  },
  tseslint.configs.recommended,
  eslintPluginSvelte.configs['flat/recommended'],
  {
    // Svelte components and rune modules (*.svelte.ts) both need the TypeScript parser.
    files: ['**/*.svelte', '**/*.svelte.ts', '**/*.svelte.js'],
    languageOptions: {
      parserOptions: {
        parser: tseslint.parser
      }
    }
  },
  {
    files: ['**/*.{tsx,svelte}'],
    rules: {
      'svelte/no-unused-svelte-ignore': 'off'
    }
  },
  eslintConfigPrettier
)
