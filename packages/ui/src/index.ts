/**
 * @vos/ui — shared, non-component exports.
 *
 * Components are only reachable through their atomic layer entry point so
 * the layer of every import is explicit and checkable:
 *
 *   import { Button } from '@vos/ui/atoms'
 *   import { ListRow } from '@vos/ui/molecules'
 *   import { Window } from '@vos/ui/organisms'
 *   import { DesktopTemplate } from '@vos/ui/templates'
 */
export { icons, type IconName } from './icons'
export type { AnchorRect, DockApp, Frame, MenuItem, SpotlightItem, StageSummary, WindowLayout } from './types'
export { uniqueBy } from './unique'
