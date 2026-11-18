/**
 * The kit registered by default: the tier-1 catalogue in two modules, one `register` call per component:
 * `kit/layout.ts` (layout, navigation, collections: wave B1a) and `kit/controls.ts` (controls, presentation, display:
 * wave B1b). Every adapter wraps an `@agentos/ui` component and passes the derived test id.
 */
import { registerControls } from './kit/controls'
import { registerLayout } from './kit/layout'
import { ComponentRegistry, createRegistry } from './registry'

/** Register the kit on a registry (so a story or test can start from it and add its own). */
export function registerStarterKit(registry: ComponentRegistry): ComponentRegistry {
  registerLayout(registry)
  return registerControls(registry)
}

/** The registry a window uses unless it is given another. */
export const defaultRegistry: ComponentRegistry = registerStarterKit(createRegistry())
