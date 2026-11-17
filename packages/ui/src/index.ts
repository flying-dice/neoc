/**
 * @agentos/ui — shared, non-component exports.
 *
 * Components are only reachable through their atomic layer entry point so
 * the layer of every import is explicit and checkable:
 *
 *   import { Button } from '@agentos/ui/atoms'
 *   import { ListRow } from '@agentos/ui/molecules'
 *   import { Window } from '@agentos/ui/organisms'
 *   import { DesktopTemplate } from '@agentos/ui/templates'
 */
export { icons, type IconName } from './icons'
export type {
  AlertAction,
  DockApp,
  Frame,
  MenuItem,
  MenuDefinition,
  StageSummary,
  WindowLayout
} from './types'
