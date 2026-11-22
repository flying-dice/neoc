export type SizeClass = 'compact' | 'regular'

/**
 * Where a length (a width or a height, in CSS px) stops being compact: the one breakpoint of the size classes, for
 * the screen the shell lays its windows out on (`WindowManager.compact`). It follows UIKit, where the same trait
 * describes a view, its window and the screen.
 */
export const COMPACT_BELOW = 560

/** The size class of one dimension. */
export const sizeClassOf = (length: number): SizeClass =>
  length < COMPACT_BELOW ? 'compact' : 'regular'
