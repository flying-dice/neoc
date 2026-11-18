/**
 * The modules app client bundles share with the shell (see SHARED_MODULES in
 * @agentos/sdk/build). App bundles don't contain Svelte, the kit or the SDK's
 * client API: their build reads them from `globalThis.__AGENTOS_SHARED__`, so
 * there is exactly one Svelte runtime, one kit and one app-context key.
 *
 * Import this first in every entry point, before any app bundle can load.
 * `sharedModules.test.ts` checks the keys match SHARED_MODULES.
 */
import * as svelte from 'svelte'
// Compiled app components call Svelte's internal client runtime, so the host
// must hand them this exact instance (one runtime). It is not used directly.
// eslint-disable-next-line svelte/no-svelte-internal
import * as svelteInternalClient from 'svelte/internal/client'
import * as svelteTransition from 'svelte/transition'
import * as svelteEasing from 'svelte/easing'
import * as svelteAnimate from 'svelte/animate'
import * as svelteMotion from 'svelte/motion'
import * as svelteStore from 'svelte/store'
import * as svelteReactivity from 'svelte/reactivity'
import * as svelteEvents from 'svelte/events'
import * as sdkAppClient from '@agentos/sdk/app/client'
import * as ui from '@agentos/ui'
import * as uiAtoms from '@agentos/ui/atoms'
import * as uiMolecules from '@agentos/ui/molecules'
import * as uiOrganisms from '@agentos/ui/organisms'
import * as uiTemplates from '@agentos/ui/templates'

export const sharedModules: Record<string, unknown> = Object.freeze({
  svelte,
  'svelte/internal/client': svelteInternalClient,
  'svelte/transition': svelteTransition,
  'svelte/easing': svelteEasing,
  'svelte/animate': svelteAnimate,
  'svelte/motion': svelteMotion,
  'svelte/store': svelteStore,
  'svelte/reactivity': svelteReactivity,
  'svelte/events': svelteEvents,
  '@agentos/sdk/app/client': sdkAppClient,
  '@agentos/ui': ui,
  '@agentos/ui/atoms': uiAtoms,
  '@agentos/ui/molecules': uiMolecules,
  '@agentos/ui/organisms': uiOrganisms,
  '@agentos/ui/templates': uiTemplates
})

Object.defineProperty(globalThis, '__AGENTOS_SHARED__', {
  value: sharedModules,
  configurable: false,
  writable: false
})
