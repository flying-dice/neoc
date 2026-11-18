/**
 * Modules an app's client bundle shares with the AgentOS client instead of
 * bundling its own copy. Svelte's runtime must be a single instance (context,
 * reactivity, effects) and the kit must be too (styles, components), so the
 * build replaces these imports with reads from `globalThis.__AGENTOS_SHARED__`,
 * which the client fills before loading any app bundle.
 *
 * Adding a module here means the client must provide it: the client's
 * `sharedModules.ts` is checked against this list by its tests.
 */
export const SHARED_GLOBAL = '__AGENTOS_SHARED__'

export const SHARED_MODULES = [
  'svelte',
  'svelte/internal/client',
  'svelte/transition',
  'svelte/easing',
  'svelte/animate',
  'svelte/motion',
  'svelte/store',
  'svelte/reactivity',
  'svelte/events',
  '@agentos/sdk/app/client',
  '@agentos/ui',
  '@agentos/ui/atoms',
  '@agentos/ui/molecules',
  '@agentos/ui/organisms',
  '@agentos/ui/templates'
] as const

/** Side-effect-only modules the client has already evaluated: they become empty. */
export const SHARED_SIDE_EFFECTS = ['svelte/internal/disclose-version'] as const

export type SharedModule = (typeof SHARED_MODULES)[number]
