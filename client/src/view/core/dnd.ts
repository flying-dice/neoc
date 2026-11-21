/**
 * Drag and drop (the common `drag` / `drop` props and the `drop` event). Pointer-event based (HTML5 DnD is
 * not touch-friendly), and every draggable has a non-pointer path:
 *
 *  - keyboard: Alt+Arrow on a focused draggable drops it on the next (Down/Right) or previous (Up/Left) target that
 *    accepts its type, in document order, and announces where it went;
 *  - menu: the node's context menu gets a "Move to..." submenu listing every target that accepts its type.
 *
 * This module is the framework-free half: validation of the props, which targets accept a drag, the order they are
 * visited in, the edge of a drop, and the registry the adapters share. The DOM half (pointer events, the ghost, hit
 * testing) is `molecules/Decorated.svelte`.
 */
import type { Json } from '../protocol'

export interface DragSpec {
  type: string
  payload?: Json
  preview?: string
}

export interface DropSpec {
  accepts: string[]
  highlight: boolean
}

const isRecord = (v: unknown): v is Record<string, unknown> =>
  typeof v === 'object' && v !== null && !Array.isArray(v)

/** `drag` prop to a spec; null for anything that is not `{type: non-empty string, ...}`. */
export function parseDrag(value: unknown): DragSpec | null {
  if (!isRecord(value) || typeof value.type !== 'string' || value.type === '') return null
  return {
    type: value.type,
    payload: value.payload as Json | undefined,
    preview: typeof value.preview === 'string' ? value.preview : undefined
  }
}

/** `drop` prop to a spec; null when it accepts nothing. */
export function parseDrop(value: unknown): DropSpec | null {
  if (!isRecord(value) || !Array.isArray(value.accepts)) return null
  const accepts = value.accepts.filter((t): t is string => typeof t === 'string' && t !== '')
  return accepts.length ? { accepts, highlight: value.highlight !== false } : null
}

export type Edge = 'top' | 'bottom' | 'leading' | 'trailing' | 'inside'

/** The `drop` event payload (the schema's `type`, `payload`, `target`). */
export interface DropEvent {
  type: string
  payload: Json | null
  target: { key: string; beforeKey: string | null; edge: Edge }
}

export interface Rect {
  x: number
  y: number
  width: number
  height: number
}

export interface DropTarget {
  /** Unique per instance: the node path plus the row key. */
  id: string
  /** The node's key (what the app addresses it by in the event). */
  key: string
  /** What it is called to a person ("To do", "Done"): the accessible name, for the Move to menu. */
  label: string
  accepts: readonly string[]
  /** Position in document order (smaller is earlier). */
  order(): number
  rect(): Rect | null
  /** Does it hold this element (the source's own column)? Such a target is not a destination of a keyboard move. */
  holds(el: Element): boolean
  /** Does it hold other draggables (a column), so a drop lands inside it rather than beside it? */
  isContainer(): boolean
  drop(event: DropEvent): void
}

const area = (r: Rect): number => r.width * r.height
const contains = (r: Rect, p: { x: number; y: number }): boolean =>
  p.x >= r.x && p.x <= r.x + r.width && p.y >= r.y && p.y <= r.y + r.height

/** The edge of a pointer drop: the middle of a container is `inside`; otherwise the nearest half of the target. */
export function pointerEdge(rect: Rect, point: { x: number; y: number }, container: boolean): Edge {
  const fx = (point.x - rect.x) / Math.max(1, rect.width)
  const fy = (point.y - rect.y) / Math.max(1, rect.height)
  if (container && fx > 0.15 && fx < 0.85 && fy > 0.15 && fy < 0.85) return 'inside'
  if (rect.width > rect.height * 2.5 && !container) return fx < 0.5 ? 'leading' : 'trailing'
  return fy < 0.5 ? 'top' : 'bottom'
}

/** What a keyboard move to `target` means: into a container, or beside an item in the direction of travel. */
export const keyboardEdge = (container: boolean, dir: 1 | -1): Edge =>
  container ? 'inside' : dir > 0 ? 'bottom' : 'top'

export class DragController {
  private readonly targets = new Map<string, DropTarget>()
  /** The drag in progress (pointer), for highlighting accepting targets. */
  active: { id: string; type: string } | null = null
  /** The target the pointer is over right now (highlighted while dragging). */
  over: string | null = null
  private listeners = new Set<() => void>()

  register(target: DropTarget): () => void {
    this.targets.set(target.id, target)
    return () => void this.targets.delete(target.id)
  }

  /** Targets that accept `type`, in document order, never the source itself. */
  candidates(type: string, sourceId?: string): DropTarget[] {
    return [...this.targets.values()]
      .filter((t) => t.id !== sourceId && t.accepts.includes(type))
      .sort((a, b) => a.order() - b.order())
  }

  /**
   * The target a keyboard move goes to: the first candidate after (`dir` 1) or the last before (-1) the source's
   * place in document order. Targets that hold the source (its column) are not destinations. Null at the ends.
   */
  step(
    type: string,
    sourceId: string,
    sourceOrder: number,
    dir: 1 | -1,
    holdsSource: (t: DropTarget) => boolean = () => false
  ): DropTarget | null {
    const list = this.candidates(type, sourceId).filter((t) => !holdsSource(t))
    if (dir > 0) return list.find((t) => t.order() > sourceOrder) ?? null
    return [...list].reverse().find((t) => t.order() < sourceOrder) ?? null
  }

  /** The innermost accepting target under a point (the smallest rect that contains it). */
  hit(type: string, point: { x: number; y: number }, sourceId?: string): DropTarget | null {
    let best: DropTarget | null = null
    let bestArea = Infinity
    for (const t of this.candidates(type, sourceId)) {
      const r = t.rect()
      if (!r || !contains(r, point)) continue
      if (area(r) < bestArea) {
        best = t
        bestArea = area(r)
      }
    }
    return best
  }

  start(id: string, type: string): void {
    this.active = { id, type }
    this.emit()
  }

  setOver(id: string | null): void {
    if (this.over === id) return
    this.over = id
    this.emit()
  }

  end(): void {
    this.active = null
    this.over = null
    this.emit()
  }

  onChange(listener: () => void): () => void {
    this.listeners.add(listener)
    return () => void this.listeners.delete(listener)
  }

  private emit(): void {
    for (const l of this.listeners) l()
  }
}
