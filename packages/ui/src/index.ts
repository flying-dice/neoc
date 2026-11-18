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
  AlertField,
  MenuEntry,
  OptionItem,
  DockApp,
  Frame,
  MenuItem,
  MenuDefinition,
  StageSummary,
  WindowLayout
} from './types'
export {
  PALETTE_TOKENS,
  cssColor,
  inkFor,
  isPaletteToken,
  normaliseHex,
  onColor,
  type PaletteToken
} from './contrast'
export {
  optionsFromExtensions,
  parseMarkdown,
  plainText,
  safeHref,
  type Block,
  type Inline,
  type MarkdownOptions
} from './markdown'
export { isContextMenuKey, moveIndex, nextEnabled, typeAhead } from './keys'
export { place, placeAtPoint, type Edge, type Placement, type Rect } from './floating'
export { normaliseDetents, type Detent } from './sheet'
export {
  addDays,
  addMonths,
  clampParts,
  dayDisabled,
  formatValue,
  fromNative,
  monthGrid,
  nativeType,
  parseValue,
  stepMinutes,
  toNative,
  type DateKind,
  type Parts
} from './datepicker'
export { initials, safeImageSrc } from './avatar'
export { trapFocus, focusables } from './focus'
