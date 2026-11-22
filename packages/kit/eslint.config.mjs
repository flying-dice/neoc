import { defineConfig } from 'eslint/config'
import eslint from '@eslint/js'
import globals from 'globals'
import tseslint from 'typescript-eslint'
import prettierRecommended from 'eslint-plugin-prettier/recommended'

// As the client's config: Prettier runs as a lint rule and only warns, so formatting never fails a build and
// `bun run format` fixes it. Everything else is an error.
const eslintConfigPrettier = {
  ...prettierRecommended,
  rules: { ...prettierRecommended.rules, 'prettier/prettier': 'warn' }
}

export default defineConfig(
  {
    // fixtures are what an app writes (React and Svelte files that are checked by their own compilers)
    ignores: ['**/node_modules', '**/dist', '**/storybook-static', 'fixtures']
  },
  eslint.configs.recommended,
  {
    languageOptions: {
      ecmaVersion: 2022,
      sourceType: 'module',
      globals: { ...globals.browser, ...globals.es2021 }
    }
  },
  tseslint.configs.recommended,
  {
    rules: {
      '@typescript-eslint/ban-ts-comment': ['error', { 'ts-ignore': 'allow-with-description' }],
      '@typescript-eslint/explicit-function-return-type': [
        'error',
        {
          allowExpressions: true,
          allowTypedFunctionExpressions: true,
          allowHigherOrderFunctions: true,
          allowIIFEs: true
        }
      ],
      '@typescript-eslint/no-empty-function': ['error', { allow: ['arrowFunctions'] }],
      '@typescript-eslint/no-explicit-any': 'error',
      '@typescript-eslint/no-non-null-assertion': 'off',
      '@typescript-eslint/no-unused-expressions': [
        'error',
        { allowShortCircuit: true, allowTaggedTemplates: true, allowTernary: true }
      ]
    }
  },
  {
    files: ['**/*.js', '**/*.mjs', '**/*.config.ts'],
    languageOptions: { globals: { ...globals.node } },
    rules: { '@typescript-eslint/explicit-function-return-type': 'off' }
  },
  {
    // the specs and the generated-manifest script run in Node
    files: ['e2e/**/*.ts'],
    languageOptions: { globals: { ...globals.node } }
  },
  eslintConfigPrettier
)
