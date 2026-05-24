import { defineConfig } from 'vitepress'
import llmstxt from 'vitepress-plugin-llms'

// https://vitepress.dev/reference/site-config
export default defineConfig({
  // Set VITEPRESS_BASE in GitLab CI/CD variables if Pages is served at a
  // subpath (e.g. /neoc/). Defaults to / for local dev and domain-root deploys.
  base: process.env.VITEPRESS_BASE ?? '/',
  vite: {
    plugins: [
      llmstxt({
        ignoreFiles: ['rfcs/template.md', 'adrs/template.md'],
      }),
    ],
  },
  srcDir: 'src',
  title: 'neoc',
  description: 'Embedded Luau runtime, Rust-backed. Sandboxed engine with a curated module surface.',
  markdown: {
    theme: {
      light: 'catppuccin-latte',
      dark: 'catppuccin-mocha',
    },
  },
  themeConfig: {
    logo: {
      light: '/logo-light.svg',
      dark: '/logo-dark.svg',
      alt: 'neoc',
    },
    siteTitle: false,

    nav: [
      { text: 'Guides', link: '/guides/' },
      {
        text: 'Modules',
        items: [
          { text: 'std:*', link: '/std/' },
          { text: 'lib:*', link: '/lib/' },
          { text: 'vnd:*', link: '/vnd/' },
        ],
      },
      { text: 'RFCs', link: '/rfcs/' },
      { text: 'ADRs', link: '/adrs/' },
      {
        text: 'LLMs',
        items: [
          { text: 'llms.txt', link: '/llms.txt', target: '_blank' },
          { text: 'llms-full.txt', link: '/llms-full.txt', target: '_blank' },
        ],
      },
    ],

    sidebar: {
      '/guides/': [
        {
          text: 'Guides',
          items: [
            { text: 'Index', link: '/guides/' },
            { text: 'Getting started', link: '/guides/getting-started' },
            { text: 'The sandboxing model', link: '/guides/sandboxing-model' },
            { text: 'The module system', link: '/guides/module-system' },
            { text: 'Error conventions', link: '/guides/error-conventions' },
          ],
        },
      ],

      '/std/': [
        {
          text: 'std:*',
          items: [
            { text: 'Overview', link: '/std/' },
          ],
        },
        {
          text: 'Reference',
          items: [
            { text: 'collections', link: '/std/reference/collections' },
            { text: 'fs', link: '/std/reference/fs' },
            { text: 'io', link: '/std/reference/io' },
            { text: 'net', link: '/std/reference/net' },
            { text: 'thread', link: '/std/reference/thread' },
            { text: 'workers', link: '/std/reference/workers' },
          ],
        },
      ],

      '/lib/': [
        {
          text: 'lib:*',
          items: [
            { text: 'Overview', link: '/lib/' },
          ],
        },
        {
          text: 'Reference',
          items: [
            { text: 'base64', link: '/lib/reference/base64' },
            { text: 'json', link: '/lib/reference/json' },
            { text: 'random', link: '/lib/reference/random' },
            { text: 'test', link: '/lib/reference/test' },
            { text: 'vcr', link: '/lib/reference/vcr' },
          ],
        },
      ],

      '/vnd/': [
        {
          text: 'vnd:*',
          items: [
            { text: 'Overview', link: '/vnd/' },
          ],
        },
        {
          text: 'HTTP and networking',
          items: [
            { text: 'hyper', link: '/vnd/reference/hyper' },
          ],
        },
        {
          text: 'Data formats',
          items: [
            { text: 'serde_json', link: '/vnd/reference/serde_json' },
            { text: 'serde_yaml', link: '/vnd/reference/serde_yaml' },
            { text: 'toml', link: '/vnd/reference/toml' },
            { text: 'quick_xml', link: '/vnd/reference/quick_xml' },
            { text: 'jsonschema', link: '/vnd/reference/jsonschema' },
          ],
        },
        {
          text: 'Databases',
          items: [
            { text: 'rusqlite', link: '/vnd/reference/rusqlite' },
            { text: 'sqlx_mysql', link: '/vnd/reference/sqlx_mysql' },
            { text: 'sqlx_postgres', link: '/vnd/reference/sqlx_postgres' },
          ],
        },
        {
          text: 'Test infrastructure',
          items: [
            { text: 'testcontainers_modules', link: '/vnd/reference/testcontainers_modules' },
          ],
        },
      ],

      '/rfcs/': [
        {
          text: 'RFCs',
          items: [
            { text: 'Index', link: '/rfcs/' },
            { text: 'Template', link: '/rfcs/template' },
          ],
        },
        {
          text: 'Drafts',
          items: [
            { text: '0001 — Sendable concurrency model', link: '/rfcs/0001-sendable-concurrency-model' },
            { text: '0003 — Feature request mechanism', link: '/rfcs/0003-feature-request-mechanism' },
            { text: '0004 — std:env', link: '/rfcs/0004-std-env' },
            { text: '0005 — std:task', link: '/rfcs/0005-std-task' },
            { text: '0006 — lib:mime', link: '/rfcs/0006-lib-mime-extension-and-filename-detection' },
            { text: '0007 — std:path', link: '/rfcs/0007-std-path' },
          ],
        },
        {
          text: 'Approved',
          items: [
            { text: '0008 — Errors guide', link: '/rfcs/0008-errors-guide' },
          ],
        },
        {
          text: 'Accepted',
          items: [
            { text: '0009 — lib:compress', link: '/rfcs/0009-lib-compress' },
            { text: '0010 — lib:csv', link: '/rfcs/0010-lib-csv' },
          ],
        },
        {
          text: 'Implemented',
          items: [
            { text: '0002 — lib:compile', link: '/rfcs/0002-lib-compile' },
          ],
        },
        {
          text: 'Superseded',
          items: [
            { text: '0000 — Spec-driven development', link: '/rfcs/0000-spec-driven-development' },
          ],
        },
      ],

      '/adrs/': [
        {
          text: 'ADRs',
          items: [
            { text: 'Index', link: '/adrs/' },
            { text: 'Template', link: '/adrs/template' },
          ],
        },
        {
          text: 'Accepted',
          items: [
            { text: '0001 — Use Luau as the embedded language', link: '/adrs/0001-use-luau-as-the-embedded-language' },
            { text: '0002 — Strip unsafe Lua stdlib', link: '/adrs/0002-strip-unsafe-lua-stdlib' },
            { text: '0003 — Three-namespace module structure', link: '/adrs/0003-three-namespace-module-structure' },
            { text: '0004 — vnd:* names match upstream', link: '/adrs/0004-vnd-module-names-match-upstream' },
            { text: '0005 — Tuple error convention', link: '/adrs/0005-tuple-error-convention' },
            { text: '0006 — Hidden async with sync-looking APIs', link: '/adrs/0006-hidden-async-with-sync-looking-apis' },
            { text: '0008 — lib:http static file server', link: '/adrs/0008-lib-http-static-file-server' },
            { text: '0009 — --eval / -e CLI flag', link: '/adrs/0009-eval-cli-flag' },
          ],
        },
        {
          text: 'Proposed',
          items: [
            { text: '0007 — std:process', link: '/adrs/0007-std-process-module' },
          ],
        },
      ],
    },

    socialLinks: [
      { icon: 'gitlab', link: 'https://gitlab.beluga-sirius.ts.net/flying-dice/neoc' },
    ],

    editLink: {
      pattern: 'https://gitlab.beluga-sirius.ts.net/flying-dice/neoc/-/edit/main/docs/src/:path',
      text: 'Edit this page on GitLab',
    },

    search: {
      provider: 'local',
    },
  },
})
